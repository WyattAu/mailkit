//! SendGrid v3 provider (`sendgrid` feature).
//!
//! Posts to [`/v3/mail/send`](https://www.twilio.com/docs/sendgrid/api-reference/mail-send/mail-send)
//! with a bearer token on the shared `reqwest` client. Provider-level
//! defaults (categories, custom args, sandbox mode, tracking toggles) apply
//! to every message sent through this provider instance.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use crate::error::EmailError;
use crate::message::EmailMessage;

use super::{MailProvider, SendReceipt};

const DEFAULT_BASE_URL: &str = "https://api.sendgrid.com";

/// Per-attachment size guard used when encoding attachments.
const MAX_ATTACHMENT_SIZE: u64 = crate::mime::DEFAULT_MAX_ATTACHMENT_SIZE;

/// SendGrid v3 mail/send provider.
///
/// # Example
///
/// ```no_run
/// use mailkit::{EmailClient, EmailMessage, SendGridProvider};
///
/// # async fn demo() -> Result<(), mailkit::EmailError> {
/// let provider = SendGridProvider::new("SG.your-api-key")
///     .with_category("transactional")
///     .with_sandbox_mode(false)
///     .with_open_tracking(true);
/// let client = EmailClient::new(provider);
///
/// let message = EmailMessage::builder()
///     .from("sender@example.com")
///     .to("recipient@example.com")
///     .subject("Hello SendGrid")
///     .html_body("<h1>Hi!</h1>")
///     .build()?;
///
/// let receipt = client.send_with_receipt(message).await?;
/// println!("SendGrid message id: {:?}", receipt.message_id);
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct SendGridProvider {
    api_key: String,
    client: reqwest::Client,
    base_url: String,
    categories: Vec<String>,
    custom_args: BTreeMap<String, String>,
    sandbox_mode: Option<bool>,
    open_tracking: Option<bool>,
    click_tracking: Option<bool>,
}

impl SendGridProvider {
    /// Create a new SendGrid provider with the given API key.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            client: reqwest::Client::new(),
            base_url: DEFAULT_BASE_URL.to_owned(),
            categories: Vec::new(),
            custom_args: BTreeMap::new(),
            sandbox_mode: None,
            open_tracking: None,
            click_tracking: None,
        }
    }

    /// Override the API base URL (for tests or gateway proxies).
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Add a category (applies to all sends; up to 10 per request allowed).
    #[must_use]
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.categories.push(category.into());
        self
    }

    /// Add a custom arg (applied to every personalization).
    #[must_use]
    pub fn with_custom_arg(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.custom_args.insert(key.into(), value.into());
        self
    }

    /// Enable or disable [sandbox mode](https://www.twilio.com/docs/sendgrid/api-reference/mail-send/mail-send#bodymail_settingsinbox)
    /// (validates without delivering). Unset by default — the account
    /// default applies.
    #[must_use]
    pub fn with_sandbox_mode(mut self, enable: bool) -> Self {
        self.sandbox_mode = Some(enable);
        self
    }

    /// Toggle open tracking. Unset by default.
    #[must_use]
    pub fn with_open_tracking(mut self, enable: bool) -> Self {
        self.open_tracking = Some(enable);
        self
    }

    /// Toggle click tracking. Unset by default.
    #[must_use]
    pub fn with_click_tracking(mut self, enable: bool) -> Self {
        self.click_tracking = Some(enable);
        self
    }
}

impl MailProvider for SendGridProvider {
    fn send<'a>(
        &'a self,
        message: &'a EmailMessage,
    ) -> Pin<Box<dyn Future<Output = Result<SendReceipt, EmailError>> + Send + 'a>> {
        Box::pin(async move {
            if message.text_body.is_none() && message.html_body.is_none() {
                return Err(EmailError::provider(
                    "sendgrid requires at least one of text_body or html_body",
                ));
            }

            let mut personalization = serde_json::json!({
                "to": message.to.iter().map(|a| serde_json::json!({"email": a})).collect::<Vec<_>>(),
            });
            if !message.cc.is_empty() {
                personalization["cc"] = serde_json::json!(
                    message
                        .cc
                        .iter()
                        .map(|a| serde_json::json!({"email": a}))
                        .collect::<Vec<_>>()
                );
            }
            if !message.bcc.is_empty() {
                personalization["bcc"] = serde_json::json!(
                    message
                        .bcc
                        .iter()
                        .map(|a| serde_json::json!({"email": a}))
                        .collect::<Vec<_>>()
                );
            }
            if !self.custom_args.is_empty() {
                personalization["custom_args"] = serde_json::json!(self.custom_args);
            }

            let mut body = serde_json::json!({
                "personalizations": [personalization],
                "from": {"email": message.from},
                "subject": message.subject,
                "content": content_parts(message),
            });

            if !self.categories.is_empty() {
                body["categories"] = serde_json::json!(self.categories);
            }

            if !message.attachments.is_empty() {
                let mut atts = Vec::with_capacity(message.attachments.len());
                for att in &message.attachments {
                    atts.push(serde_json::json!({
                        "content": crate::mime::attachment_base64(att, MAX_ATTACHMENT_SIZE).await?,
                        "filename": att.filename,
                        "type": att.content_type,
                        "disposition": "attachment",
                    }));
                }
                body["attachments"] = serde_json::json!(atts);
            }

            let mut mail_settings = serde_json::Map::new();
            if let Some(enable) = self.sandbox_mode {
                mail_settings.insert("sandbox_mode".into(), serde_json::json!({"enable": enable}));
            }
            if !mail_settings.is_empty() {
                body["mail_settings"] = serde_json::Value::Object(mail_settings);
            }

            let mut tracking = serde_json::Map::new();
            if let Some(enable) = self.open_tracking {
                tracking.insert(
                    "open_tracking".into(),
                    serde_json::json!({"enable": enable}),
                );
            }
            if let Some(enable) = self.click_tracking {
                tracking.insert(
                    "click_tracking".into(),
                    serde_json::json!({"enable": enable}),
                );
            }
            if !tracking.is_empty() {
                body["tracking_settings"] = serde_json::Value::Object(tracking);
            }

            let url = format!("{}/v3/mail/send", self.base_url.trim_end_matches('/'));
            let resp = self
                .client
                .post(url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&body)
                .send()
                .await
                .map_err(|e| EmailError::provider(e.to_string()))?;

            let status = resp.status();
            let headers = resp.headers().clone();
            let text = resp.text().await.unwrap_or_default();
            if !status.is_success() {
                return Err(EmailError::provider(format!(
                    "sendgrid returned {status}: {}",
                    if text.is_empty() { "<no body>" } else { &text }
                )));
            }

            let message_id = headers
                .get("X-Message-Id")
                .and_then(|v| v.to_str().ok().map(str::to_owned));

            Ok(SendReceipt::new("sendgrid", message_id))
        })
    }

    fn name(&self) -> &str {
        "sendgrid"
    }
}

fn content_parts(message: &EmailMessage) -> serde_json::Value {
    let mut parts = Vec::with_capacity(2);
    // Plain text first: SendGrid docs recommend text before html.
    if let Some(text) = &message.text_body {
        parts.push(serde_json::json!({"type": "text/plain", "value": text}));
    }
    if let Some(html) = &message.html_body {
        parts.push(serde_json::json!({"type": "text/html", "value": html}));
    }
    serde_json::json!(parts)
}
