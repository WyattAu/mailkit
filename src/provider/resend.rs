//! Resend HTTP API provider (`resend` feature).

use std::future::Future;
use std::pin::Pin;

use crate::error::EmailError;
use crate::message::EmailMessage;

use super::{MailProvider, SendReceipt};

const DEFAULT_BASE_URL: &str = "https://api.resend.com";

/// Resend HTTP API provider.
///
/// Posts to `/emails` with a bearer token. Attachments are sent as base64
/// content; file-path attachments are streamed (with the size guard).
#[derive(Clone)]
pub struct ResendProvider {
    api_key: String,
    client: reqwest::Client,
    base_url: String,
}

impl ResendProvider {
    /// Create a new Resend provider with the given API key.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            client: reqwest::Client::new(),
            base_url: DEFAULT_BASE_URL.to_owned(),
        }
    }

    /// Override the API base URL (for tests or gateway proxies).
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

impl MailProvider for ResendProvider {
    fn send<'a>(
        &'a self,
        message: &'a EmailMessage,
    ) -> Pin<Box<dyn Future<Output = Result<SendReceipt, EmailError>> + Send + 'a>> {
        Box::pin(async move {
            let mut body = serde_json::json!({
                "from": message.from,
                "to": message.to,
                "subject": message.subject,
                "html": message.html_body.as_deref().unwrap_or_default(),
                "text": message.text_body.as_deref().unwrap_or_default(),
            });

            if !message.cc.is_empty() {
                body["cc"] = serde_json::json!(message.cc);
            }

            if !message.bcc.is_empty() {
                body["bcc"] = serde_json::json!(message.bcc);
            }

            if !message.attachments.is_empty() {
                let mut atts = Vec::with_capacity(message.attachments.len());
                for att in &message.attachments {
                    atts.push(serde_json::json!({
                        "filename": att.filename,
                        "content_type": att.content_type,
                        "content": crate::mime::attachment_base64(att, crate::mime::DEFAULT_MAX_ATTACHMENT_SIZE).await?,
                    }));
                }
                body["attachments"] = serde_json::json!(atts);
            }

            let url = format!("{}/emails", self.base_url.trim_end_matches('/'));
            let resp = self
                .client
                .post(url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&body)
                .send()
                .await
                .map_err(|e| EmailError::provider(e.to_string()))?;

            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_else(|_| "<no body>".into());
                return Err(EmailError::provider(format!(
                    "resend returned {status}: {text}"
                )));
            }

            let message_id = resp
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|v| v.get("id").and_then(|id| id.as_str().map(str::to_owned)));

            Ok(SendReceipt::new("resend", message_id))
        })
    }

    fn name(&self) -> &str {
        "resend"
    }
}
