use email_address::EmailAddress;
use serde::Serialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::signal;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

#[derive(Debug, Serialize)]
struct NewEmail {
    from: EmailAddress,
    to: EmailAddress,
    subject: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl NewEmail {
    fn from_raw_message(from: EmailAddress, to: EmailAddress, body_lines: Vec<String>) -> Self {
        let mut headers = Vec::new();
        let mut body = String::new();
        let mut parsing_headers = true;
        for line in body_lines {
            if parsing_headers {
                if line.is_empty() {
                    parsing_headers = false;
                    continue;
                }

                if let Some((key, value)) = line.split_once(':') {
                    headers.push((key.trim().to_string(), value.trim().to_string()));
                } else {
                    // If the line doesn't contain a colon, treat it as a continuation of the previous header
                    if let Some(last_header) = headers.last_mut() {
                        last_header.1.push_str(&format!("\n{line}"));
                    } else {
                        // If there are no headers yet, just push the line as a header
                        headers.push((line.to_string(), String::new()));
                    }
                }
            } else {
                body.push_str(&line);
                body.push('\n');
            }
        }

        let subject = headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("Subject"))
            .map_or(String::new(), |(_, value)| value.clone());

        Self {
            from,
            to,
            subject,
            headers,
            body,
        }
    }

    async fn save(&self, db: &sqlx::Pool<sqlx::Postgres>) -> Result<(), sqlx::Error> {
        let mut tx = db.begin().await?;

        let email_id = sqlx::query!(
            r#"INSERT INTO emails ("from", "to", subject, body) VALUES ($1, $2, $3, $4) RETURNING id"#,
            self.from.to_string(),
            self.to.to_string(),
            self.subject,
            self.body
        )
        .fetch_one(&mut *tx)
        .await?
        .id;

        if !self.headers.is_empty() {
            let mut query =
                String::from("INSERT INTO email_headers (email_id, key, value) VALUES ");

            for (i, _) in self.headers.iter().enumerate() {
                if i > 0 {
                    query.push_str(", ");
                }
                query.push_str(&format!("(${}, ${}, ${})", i * 3 + 1, i * 3 + 2, i * 3 + 3));
            }

            let mut query_builder = sqlx::query(&query);
            for (key, value) in &self.headers {
                query_builder = query_builder.bind(email_id).bind(key).bind(value);
            }
            query_builder.execute(&mut *tx).await?;
        }

        tx.commit().await?;
        Ok(())
    }
}

enum SmtpState {
    Start,
    MailFrom,
    RcptTo,
    Data,
    End,
}

struct SmtpHandler {
    db: sqlx::Pool<sqlx::Postgres>,

    from: EmailAddress,
    to: EmailAddress,
    body: Vec<String>,
    write_stream: OwnedWriteHalf,
    state: SmtpState,
}

impl SmtpHandler {
    fn new(write_stream: OwnedWriteHalf, db: sqlx::Pool<sqlx::Postgres>) -> Self {
        Self {
            db,

            from: EmailAddress::new_unchecked(""),
            to: EmailAddress::new_unchecked(""),
            body: Vec::new(),
            write_stream,
            state: SmtpState::Start,
        }
    }

    async fn handle(mut self, read_stream: OwnedReadHalf) {
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
                    if let Err(e) = email.save(&self.db).await {
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    sqlx::migrate!("./migrations");

    let pg_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await?;

    let port: u16 = std::env::var("SMTP_PORT")
        .unwrap_or_else(|_| "2525".to_string())
        .parse()
        .expect("SMTP_PORT must be a valid u16");

    let listener = TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    let active_connections = Arc::new(RwLock::new(HashMap::<SocketAddr, JoinHandle<()>>::new()));

    println!("Listening on {}", listener.local_addr()?);
    println!("Press Ctrl+C to stop the server");

    let active_connections_clone = active_connections.clone();

    let accept_task = tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((socket, addr)) => {
                    println!("Accepted connection from {addr}");
                    let (read_stream, write_stream) = socket.into_split();
                    let handler = SmtpHandler::new(write_stream, pg_pool.clone());

                    let active_connections_clone_clone = active_connections_clone.clone();
                    let handle = tokio::spawn(async move {
                        handler.handle(read_stream).await;
                        println!("Connection from {addr} closed");
                        active_connections_clone_clone.write().await.remove(&addr);
                    });

                    active_connections_clone.write().await.insert(addr, handle);
                }
                Err(e) => {
                    eprintln!("Failed to accept connection: {e}");
                }
            }
        }
    });

    signal::ctrl_c().await?;
    println!("\nShutting down server...");

    accept_task.abort();

    let mut connections = active_connections.write().await;
    for handle in connections.values_mut() {
        handle
            .await
            .map_err(|e| eprintln!("Error joining task: {e:?}"))
            .ok();
    }

    println!("Server shutdown complete");
    Ok(())
}
