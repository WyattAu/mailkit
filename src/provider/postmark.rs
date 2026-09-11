//! Postmark provider (`postmark` feature).
//!
//! Posts to the [`/email`](https://postmarkapp.com/developer/api/email-api#send-a-single-email)
//! endpoint with a server token on the shared `reqwest` client.
//! Provider-level defaults (message stream, tag, metadata) apply to every
//! message sent through this provider instance.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use crate::error::EmailError;
use crate::message::EmailMessage;

use super::{MailProvider, SendReceipt};

const DEFAULT_BASE_URL: &str = "https://api.postmarkapp.com";
const DEFAULT_MESSAGE_STREAM: &str = "outbound";

/// Per-attachment size guard used when encoding attachments.
const MAX_ATTACHMENT_SIZE: u64 = crate::mime::DEFAULT_MAX_ATTACHMENT_SIZE;

/// Postmark send-email provider.
///
/// # Example
///
/// ```no_run
/// use mailkit::{EmailClient, EmailMessage, PostmarkProvider};
///
/// # async fn demo() -> Result<(), mailkit::EmailError> {
/// let provider = PostmarkProvider::new("your-server-token")
///     .with_message_stream("broadcasts")
///     .with_metadata("tenant", "acme");
/// let client = EmailClient::new(provider);
///
/// let message = EmailMessage::builder()
///     .from("sender@example.com")
///     .to("recipient@example.com")
///     .subject("Hello Postmark")
///     .text_body("Sent via Postmark.")
///     .build()?;
///
/// let receipt = client.send_with_receipt(message).await?;
/// println!("Postmark message id: {:?}", receipt.message_id);
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct PostmarkProvider {
    server_token: String,
    client: reqwest::Client,
    base_url: String,
    message_stream: String,
    tag: Option<String>,
    metadata: BTreeMap<String, String>,
}

impl PostmarkProvider {
    /// Create a new Postmark provider with the given server token.
    pub fn new(server_token: impl Into<String>) -> Self {
        Self {
            server_token: server_token.into(),
            client: reqwest::Client::new(),
            base_url: DEFAULT_BASE_URL.to_owned(),
            message_stream: DEFAULT_MESSAGE_STREAM.to_owned(),
            tag: None,
            metadata: BTreeMap::new(),
        }
    }

    /// Override the API base URL (for tests or gateway proxies).
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Set the message stream (default `outbound`).
    #[must_use]
    pub fn with_message_stream(mut self, stream: impl Into<String>) -> Self {
        self.message_stream = stream.into();
        self
    }

    /// Tag every message sent through this provider (for bounce reporting).
    #[must_use]
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// Add a metadata entry attached to every message (up to 10 per request).
    #[must_use]
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

impl MailProvider for PostmarkProvider {
    fn send<'a>(
        &'a self,
        message: &'a EmailMessage,
    ) -> Pin<Box<dyn Future<Output = Result<SendReceipt, EmailError>> + Send + 'a>> {
        Box::pin(async move {
            let mut body = serde_json::json!({
                "From": message.from,
                "To": message.to.join(", "),
                "Subject": message.subject,
                "MessageStream": self.message_stream,
            });
            if let Some(text) = &message.text_body {
                body["TextBody"] = serde_json::json!(text);
            }
            if let Some(html) = &message.html_body {
                body["HtmlBody"] = serde_json::json!(html);
            }
            if !message.cc.is_empty() {
                body["Cc"] = serde_json::json!(message.cc.join(", "));
            }
            if !message.bcc.is_empty() {
                body["Bcc"] = serde_json::json!(message.bcc.join(", "));
            }
            if let Some(tag) = &self.tag {
                body["Tag"] = serde_json::json!(tag);
            }
            if !self.metadata.is_empty() {
                body["Metadata"] = serde_json::json!(self.metadata);
            }

            if !message.attachments.is_empty() {
                let mut atts = Vec::with_capacity(message.attachments.len());
                for att in &message.attachments {
                    atts.push(serde_json::json!({
                        "Name": att.filename,
                        "Content": crate::mime::attachment_base64(att, MAX_ATTACHMENT_SIZE).await?,
                        "ContentType": att.content_type,
                    }));
                }
                body["Attachments"] = serde_json::json!(atts);
            }

            let url = format!("{}/email", self.base_url.trim_end_matches('/'));
            let resp = self
                .client
                .post(url)
                .header("X-Postmark-Server-Token", &self.server_token)
                .header("Accept", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| EmailError::provider(e.to_string()))?;

            let status = resp.status();
            let text = resp.text().await.unwrap_or_else(|_| "<no body>".into());

            let parsed: serde_json::Value =
                serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
            // Postmark signals application errors with an ErrorCode field
            // even alongside HTTP 200 on legacy routes.
            if let Some(code) = parsed.get("ErrorCode").and_then(serde_json::Value::as_i64) {
                if code != 0 {
                    let msg = parsed
                        .get("Message")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("unknown postmark error");
                    return Err(EmailError::provider(format!(
                        "postmark error {code}: {msg}"
                    )));
                }
            }
            if !status.is_success() {
                return Err(EmailError::provider(format!(
                    "postmark returned {status}: {text}"
                )));
            }

            let message_id = parsed
                .get("MessageID")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);

            Ok(SendReceipt::new("postmark", message_id))
        })
    }

    fn name(&self) -> &str {
        "postmark"
    }
}
