use email_address::EmailAddress;
use serde::Serialize;

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct NewEmail {
    pub from: EmailAddress,
    pub to: EmailAddress,
    pub subject: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl NewEmail {
    pub fn from_raw_message(from: EmailAddress, to: EmailAddress, body_lines: Vec<String>) -> Self {
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
                body.push_str("\r\n");
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
}
