use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::signal;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

enum SmtpState {
    Start,
    MailFrom,
    RcptTo,
    Data,
    End,
}

struct SmtpHandler {
    from: String,
    to: String,
    body: Vec<String>,
    write_stream: OwnedWriteHalf,
    state: SmtpState,
}

impl SmtpHandler {
    fn new(write_stream: OwnedWriteHalf) -> Self {
        SmtpHandler {
            from: String::new(),
            to: String::new(),
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
                    self.from = line[10..].trim().to_string();
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
                    self.to = line[8..].trim().to_string();
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
                    // TODO: actually store the email in inbox
                    println!(
                        "Received email from: {}, to: {}, body: {:?}",
                        self.from, self.to, self.body
                    );
                    if !self
                        .write("250 OK: Message accepted for delivery\r\n")
                        .await
                    {
                        return Some(false);
                    }

                    return Some(true);
                }

                self.body.push(line.to_string());
            }
        }

        None
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:2522").await?;
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
                    let handler = SmtpHandler::new(write_stream);

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
