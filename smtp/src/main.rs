use crate::handler::SmtpHandler;
use crate::persistor::SqlxPersistor;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::signal;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

mod email;
mod handler;
mod persistor;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    sqlx::migrate!("./migrations");

    let pg_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await?;
    let persistor = SqlxPersistor::new(pg_pool.clone());

    let port: u16 = std::env::var("SMTP_PORT")
        .unwrap_or_else(|_| "2525".to_string())
        .parse()
        .expect("SMTP_PORT must be a valid u16");

    let listener = TcpListener::bind(format!("localhost:{port}")).await?;
    let active_connections = Arc::new(RwLock::new(HashMap::<SocketAddr, JoinHandle<()>>::new()));

    println!("Listening on localhost:{port}");
    println!("Press Ctrl+C to stop the server");

    let active_connections_clone = active_connections.clone();

    let accept_task = tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((socket, addr)) => {
                    println!("Accepted connection from {addr}");
                    let (read_stream, write_stream) = socket.into_split();
                    let handler = SmtpHandler::new(write_stream, persistor.clone());

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
