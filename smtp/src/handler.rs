use crate::email::NewEmail;
use crate::persistor::SmtpPersistor;
use email_address::EmailAddress;
use std::str::FromStr;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

enum SmtpState {
    Start,
    MailFrom,
    RcptTo,
    Data,
    End,
}

pub struct SmtpHandler<P: SmtpPersistor, W: AsyncWrite + Unpin> {
    persistor: P,

    from: EmailAddress,
    to: EmailAddress,
    body: Vec<String>,
    write_stream: W,
    state: SmtpState,
}

impl<P: SmtpPersistor, W: AsyncWrite + Unpin> SmtpHandler<P, W> {
    pub fn new(write_stream: W, persistor: P) -> Self {
        Self {
            persistor,

            from: EmailAddress::new_unchecked(""),
            to: EmailAddress::new_unchecked(""),
            body: Vec::new(),
            write_stream,
            state: SmtpState::Start,
        }
    }

    pub async fn handle(mut self, read_stream: impl AsyncRead + Unpin) {
        if !self.write("220 smt.example.com ESMTP Remail\r\n").await {
            self.shutdown().await;
            return;
        }

        let mut lines = BufReader::new(read_stream).lines();

        loop {
            let line = lines.next_line().await;
            match line {
                Ok(Some(line)) => {
                    let line = line.trim();
                    if let Some(success) = self.handle_line(line).await {
                        if !success {
                            eprintln!("Error handling line: {line}");
                        }
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    eprintln!("Error reading line: {e}");
                    self.shutdown().await;
                    return;
                }
            }
        }

        self.shutdown().await;
    }

    async fn shutdown(&mut self) {
        if let Err(e) = self.write_stream.shutdown().await {
            eprintln!("Error shutting down stream: {e}");
        }
    }

    async fn write(&mut self, response: &str) -> bool {
        self.write_stream
            .write(response.as_bytes())
            .await
            .map(|_| true)
            .unwrap_or_else(|e| {
                eprintln!("Error writing to stream: {e}");
                false
            })
    }

    async fn handle_line(&mut self, line: &str) -> Option<bool> {
        match self.state {
            SmtpState::Start => {
                if line.len() < 4 {
                    self.write("500 Unrecognized command\r\n").await;
                    return Some(false);
                }
                let line = line[..4].to_uppercase();
                if line == "HELO" || line == "EHLO" {
                    self.state = SmtpState::MailFrom;
                    if !self.write("250 Hello\r\n").await {
                        return Some(false);
                    }
                } else {
                    self.write("500 Unrecognized command\r\n").await;
                    return Some(false);
                }
            }
            SmtpState::MailFrom => {
                if line.len() < 10 {
                    self.write("500 Unrecognized command\r\n").await;
                    return Some(false);
                }
                if line[..10].to_uppercase() == "MAIL FROM:" {
                    let from = line[10..]
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .strip_prefix('<')
                        .and_then(|s| s.strip_suffix('>'))
                        .unwrap_or("")
                        .to_string();

                    match EmailAddress::from_str(&from) {
                        Ok(email) => self.from = email,
                        Err(_) => {
                            self.write("501 Syntax error in parameters or arguments\r\n")
                                .await;
                            return Some(false);
                        }
                    }

                    if !self.write("250 OK\r\n").await {
                        return Some(false);
                    }

                    self.state = SmtpState::RcptTo;
                } else {
                    self.write("503 Bad sequence of commands\r\n").await;
                    return Some(false);
                }
            }
            SmtpState::RcptTo => {
                if line.len() < 8 {
                    self.write("500 Unrecognized command\r\n").await;
                    return Some(false);
                }
                if line[..8].to_uppercase() == "RCPT TO:" {
                    let to = line[8..]
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .strip_prefix('<')
                        .and_then(|s| s.strip_suffix('>'))
                        .unwrap_or("")
                        .to_string();
                    match EmailAddress::from_str(&to) {
                        Ok(email) => self.to = email,
                        Err(_) => {
                            self.write("501 Syntax error in parameters or arguments\r\n")
                                .await;
                            return Some(false);
                        }
                    }

                    if !self.write("250 OK\r\n").await {
                        return Some(false);
                    }

                    self.state = SmtpState::Data;
                } else {
                    self.write("503 Bad sequence of commands\r\n").await;
                    return Some(false);
                }
            }
            SmtpState::Data => {
                if line.to_uppercase() == "DATA" {
                    if !self
                        .write("354 Start mail input; end with <CRLF>.<CRLF>\r\n")
                        .await
                    {
                        return Some(false);
                    }

                    self.state = SmtpState::End
                } else {
                    self.write("503 Bad sequence of commands\r\n").await;
                    return Some(false);
                }
            }
            SmtpState::End => {
                if line == "." {
                    let email = NewEmail::from_raw_message(
                        self.from.clone(),
                        self.to.clone(),
                        self.body.clone(),
                    );
                    if let Err(e) = self.persistor.persist_email(&email).await {
                        eprintln!("Error saving email: {e}");
                        if !self.write("550 Internal server error\r\n").await {
                            return Some(false);
                        }
                        return Some(false);
                    }

                    if !self
                        .write("250 OK: Message accepted for delivery\r\n")
                        .await
                    {
                        return Some(false);
                    }

                    return Some(true);
                }

                let line_to_push = if let Some(line) = line.strip_prefix(".") {
                    // Section 4.5.2 of RFC 5321 states that lines starting with a dot
                    // should have the dot removed when they are part of the message body.
                    // This is to avoid confusion with the end of data marker.
                    // So we push the line without the leading dot.
                    line.to_string()
                } else {
                    line.to_string()
                };

                self.body.push(line_to_push);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::email::NewEmail;
    use crate::persistor::SmtpPersistor;

    #[derive(Default)]
    struct MockSmtpPersistor {}
    impl SmtpPersistor for MockSmtpPersistor {
        async fn persist_email(&self, _email: &NewEmail) -> Result<(), sqlx::Error> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_smtp_handler() {
        let mut _mock_persistor = MockSmtpPersistor::default();
    }
}
