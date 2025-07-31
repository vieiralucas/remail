use axum::{Json, Router, extract::State, response::IntoResponse};
use remail_types::Email;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use uuid::Uuid;

async fn list_emails(db: &sqlx::Pool<sqlx::Postgres>) -> Result<Vec<Email>, sqlx::Error> {
    let emails = sqlx::query!(
        r#"
        SELECT id, "from", "to", subject, body, created_at, updated_at
        FROM emails
        ORDER BY created_at DESC
        "#
    )
    .fetch_all(db)
    .await?;

    let email_ids: Vec<Uuid> = emails.iter().map(|e| e.id).collect();

    let headers = if !email_ids.is_empty() {
        sqlx::query!(
            r#"
            SELECT email_id, key, value
            FROM email_headers
            WHERE email_id = ANY($1)
            ORDER BY email_id, key
            "#,
            &email_ids
        )
        .fetch_all(db)
        .await?
    } else {
        Vec::new()
    };

    let mut headers_by_email: std::collections::HashMap<Uuid, Vec<(String, String)>> =
        std::collections::HashMap::new();

    for header in headers {
        headers_by_email
            .entry(header.email_id)
            .or_default()
            .push((header.key, header.value));
    }

    let result: Vec<Email> = emails
        .into_iter()
        .map(|email| Email {
            id: email.id,
            from: email.from,
            to: email.to,
            subject: email.subject,
            headers: headers_by_email.remove(&email.id).unwrap_or_default(),
            body: email.body,
            created_at: chrono::DateTime::from_timestamp(
                email.created_at.unix_timestamp(),
                email.created_at.nanosecond(),
            )
            .unwrap_or_default(),
            updated_at: chrono::DateTime::from_timestamp(
                email.updated_at.unix_timestamp(),
                email.updated_at.nanosecond(),
            )
            .unwrap_or_default(),
        })
        .collect();

    Ok(result)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    sqlx::migrate!("../smtp/migrations");

    let pg_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await?;

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _request_head| {
            let origin_str = origin.to_str().unwrap_or("");
            origin_str.starts_with("http://localhost:")
        }))
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/readyz", axum::routing::get(|| async { "OK" }))
        .route("/livez", axum::routing::get(|| async { "OK" }))
        .route(
            "/v1/emails",
            axum::routing::get(|State(db): State<sqlx::Pool<sqlx::Postgres>>| async move {
                match list_emails(&db).await {
                    Ok(emails) => Json(emails).into_response(),
                    Err(e) => {
                        eprintln!("Error fetching emails: {e}");
                        (
                            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                            "Internal Server Error",
                        )
                            .into_response()
                    }
                }
            }),
        )
        .layer(cors)
        .with_state(pg_pool);

    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse()
        .expect("PORT must be a valid u16");

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("Failed to bind TCP listener");

    println!("Listening on http://0.0.0.0:{port}");
    axum::serve(listener, app)
        .await
        .expect("Failed to start server");

    Ok(())
}
