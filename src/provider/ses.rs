//! AWS SES v2 provider (`ses` feature).
//!
//! Calls the SES v2 [`SendEmail`] REST-JSON API
//! (`POST` to `https://email.<region>.amazonaws.com/v2/email/outbound-emails`)
//! with a hand-rolled AWS Signature Version 4 ([`crate::sigv4`]) — no
//! `aws-sdk-sesv2` dependency.
//!
//! Messages without attachments are sent as SES `Simple` content; messages
//! with attachments are rendered with the [`crate::mime`] builder and sent
//! as SES `Raw` MIME.
//!
//! [`SendEmail`]: https://docs.aws.amazon.com/ses/latest/APIReference-V2/operations/SendEmail.html

use std::future::Future;
use std::pin::Pin;

use crate::error::EmailError;
use crate::message::EmailMessage;

use super::{MailProvider, SendReceipt};

/// AWS SES v2 email provider (hand-rolled SigV4, no AWS SDK dependency).
///
/// # Example
///
/// ```no_run
/// use mailkit::{EmailClient, EmailMessage, SesProvider};
///
/// # async fn demo() -> Result<(), mailkit::EmailError> {
/// let provider = SesProvider::new(
///     "us-east-1",
///     "AKIA...",
///     "secret",
///     "sender@example.com",
/// );
/// let client = EmailClient::new(provider);
///
/// let message = EmailMessage::builder()
///     .from("sender@example.com")
///     .to("recipient@example.com")
///     .subject("Hello SES")
///     .text_body("Sent with SigV4 signing.")
///     .build()?;
///
/// let receipt = client.send_with_receipt(message).await?;
/// println!("SES message id: {:?}", receipt.message_id);
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct SesProvider {
    client: reqwest::Client,
    region: String,
    access_key: String,
    secret_key: String,
    session_token: Option<String>,
    from: String,
    configuration_set: Option<String>,
    endpoint: Option<String>,
}

impl SesProvider {
    /// Create a new SES provider.
    ///
    /// - `region`: AWS region (e.g. `us-east-1`).
    /// - `access_key` / `secret_key`: long-lived IAM credentials or those
    ///   obtained from an STS call; pair with [`SesProvider::with_session_token`]
    ///   for temporary credentials.
    /// - `from`: verified from address.
    pub fn new(
        region: impl Into<String>,
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        from: impl Into<String>,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            region: region.into(),
            access_key: access_key.into(),
            secret_key: secret_key.into(),
            session_token: None,
            from: from.into(),
            configuration_set: None,
            endpoint: None,
        }
    }

    /// Attach a session token (temporary/STS credentials).
    #[must_use]
    pub fn with_session_token(mut self, token: impl Into<String>) -> Self {
        self.session_token = Some(token.into());
        self
    }

    /// Send through a SES configuration set.
    #[must_use]
    pub fn with_configuration_set(mut self, name: impl Into<String>) -> Self {
        self.configuration_set = Some(name.into());
        self
    }

    /// Override the API endpoint (for tests or VPC endpoints). Accepts a
    /// full URL such as `http://localhost:8080` (the default path
    /// `/v2/email/outbound-emails` is appended if the URL has no path).
    #[must_use]
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    fn host(&self) -> String {
        format!("email.{}.amazonaws.com", self.region)
    }

    fn url(&self) -> String {
        match &self.endpoint {
            Some(base) => {
                let has_path = base
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .contains('/');
                if has_path {
                    base.to_owned()
                } else {
                    format!("{}/v2/email/outbound-emails", base.trim_end_matches('/'))
                }
            }
            None => format!("https://{}/v2/email/outbound-emails", self.host()),
        }
    }
}

impl MailProvider for SesProvider {
    fn send<'a>(
        &'a self,
        message: &'a EmailMessage,
    ) -> Pin<Box<dyn Future<Output = Result<SendReceipt, EmailError>> + Send + 'a>> {
        Box::pin(async move {
            // Fall back to the configured default sender for messages built
            // without a from address.
            let from = if message.from.is_empty() {
                self.from.clone()
            } else {
                message.from.clone()
            };
            let mut body = serde_json::json!({
                "FromEmailAddress": from,
                "Destination": {
                    "ToAddresses": message.to,
                    "CcAddresses": message.cc,
                    "BccAddresses": message.bcc,
                },
            });
            if let Some(cs) = &self.configuration_set {
                body["ConfigurationSetName"] = serde_json::json!(cs);
            }

            if message.attachments.is_empty() {
                let mut simple = serde_json::json!({
                    "Subject": {"Data": message.subject, "Charset": "UTF-8"},
                    "Body": {},
                });
                if let Some(text) = &message.text_body {
                    simple["Body"]["Text"] = serde_json::json!({"Data": text, "Charset": "UTF-8"});
                }
                if let Some(html) = &message.html_body {
                    simple["Body"]["Html"] = serde_json::json!({"Data": html, "Charset": "UTF-8"});
                }
                body["Content"] = serde_json::json!({"Simple": simple});
            } else {
                // Full MIME via the built-in builder, sent as SES Raw.
                let mime = crate::mime::MimeBuilder::from_message(message)
                    .build()
                    .await?;
                body["Content"] = serde_json::json!({
                    "Raw": {"Data": crate::base64::encode(mime.as_bytes())},
                });
            }

            let payload = serde_json::to_vec(&body)?;
            let url = self.url();
            let host = match &self.endpoint {
                Some(base) => base
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .split('/')
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
                None => self.host(),
            };

            let amz_date = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
            let mut extra: Vec<(&str, &str)> = Vec::new();
            if let Some(token) = &self.session_token {
                extra.push(("x-amz-security-token", token.as_str()));
            }
            let signed = crate::sigv4::sign_request(
                "POST",
                &host,
                "/v2/email/outbound-emails",
                "",
                "application/json",
                &extra,
                &payload,
                &amz_date,
                &self.access_key,
                &self.secret_key,
                &self.region,
                "ses",
            );

            let mut request = self
                .client
                .post(&url)
                .header("Content-Type", "application/json")
                .header("Authorization", signed.authorization)
                .header("x-amz-date", signed.amz_date);
            if let Some(token) = &self.session_token {
                request = request.header("x-amz-security-token", token);
            }

            let resp = request
                .body(payload)
                .send()
                .await
                .map_err(|e| EmailError::provider(e.to_string()))?;

            let status = resp.status();
            let text = resp.text().await.unwrap_or_else(|_| "<no body>".into());
            if !status.is_success() {
                return Err(EmailError::provider(format!(
                    "ses returned {status}: {text}"
                )));
            }

            let message_id = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| {
                    v.get("MessageId")
                        .and_then(|id| id.as_str().map(str::to_owned))
                });

            Ok(SendReceipt::new("ses", message_id))
        })
    }

    fn name(&self) -> &str {
        "ses"
    }
}
