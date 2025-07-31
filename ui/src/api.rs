use remail_types::Email;

const API_BASE_URL: &str = "http://localhost:3000";

pub struct ApiClient {
    client: reqwest::Client,
}

impl Default for ApiClient {
    fn default() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl ApiClient {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn list_emails(&self) -> Result<Vec<Email>, Box<dyn std::error::Error>> {
        let response = self
            .client
            .get(format!("{API_BASE_URL}/v1/emails"))
            .send()
            .await?;

        if response.status().is_success() {
            let emails: Vec<Email> = response.json().await?;
            Ok(emails)
        } else {
            let error_text = response.text().await?;
            Err(format!("API error: {error_text}").into())
        }
    }
}
