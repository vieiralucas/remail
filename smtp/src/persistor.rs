use crate::email::NewEmail;

pub trait SmtpPersistor {
    async fn persist_email(&self, email: &NewEmail) -> Result<(), sqlx::Error>;
}

#[derive(Clone)]
pub struct SqlxPersistor {
    db: sqlx::Pool<sqlx::Postgres>,
}

impl SqlxPersistor {
    pub fn new(db: sqlx::Pool<sqlx::Postgres>) -> Self {
        Self { db }
    }
}

impl SmtpPersistor for SqlxPersistor {
    async fn persist_email(&self, email: &NewEmail) -> Result<(), sqlx::Error> {
        let mut tx = self.db.begin().await?;

        let email_id = sqlx::query!(
            r#"INSERT INTO emails ("from", "to", subject, body) VALUES ($1, $2, $3, $4) RETURNING id"#,
            email.from.to_string(),
            email.to.to_string(),
            email.subject,
            email.body
        )
        .fetch_one(&mut *tx)
        .await?
        .id;

        if !email.headers.is_empty() {
            let mut query =
                String::from("INSERT INTO email_headers (email_id, key, value) VALUES ");

            for (i, _) in email.headers.iter().enumerate() {
                if i > 0 {
                    query.push_str(", ");
                }
                query.push_str(&format!("(${}, ${}, ${})", i * 3 + 1, i * 3 + 2, i * 3 + 3));
            }

            let mut query_builder = sqlx::query(&query);
            for (key, value) in &email.headers {
                query_builder = query_builder.bind(email_id).bind(key).bind(value);
            }
            query_builder.execute(&mut *tx).await?;
        }

        tx.commit().await?;
        Ok(())
    }
}
