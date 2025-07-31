use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Email {
    pub id: Uuid,
    pub from: String,
    pub to: String,
    pub subject: Option<String>,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
