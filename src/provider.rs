use crate::error::EmailError;
use crate::message::EmailMessage;

/// Trait for email transport providers.
pub trait EmailProvider: Send + Sync {
    /// Send an email message.
    fn send(
        &self,
        message: &EmailMessage,
    ) -> impl std::future::Future<Output = Result<(), EmailError>> + Send;
}

/// Resend HTTP API provider.
pub struct ResendProvider {
    api_key: String,
    client: reqwest::Client,
}

impl ResendProvider {
    /// Create a new Resend provider with the given API key.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            client: reqwest::Client::new(),
        }
    }
}

impl EmailProvider for ResendProvider {
    fn send(
        &self,
        message: &EmailMessage,
    ) -> impl std::future::Future<Output = Result<(), EmailError>> + Send {
        let api_key = self.api_key.clone();
        let client = self.client.clone();
        let message = message.clone();

        async move {
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

            let resp = client
                .post("https://api.resend.com/emails")
                .header("Authorization", format!("Bearer {api_key}"))
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

            Ok(())
        }
    }
}

#[cfg(feature = "smtp")]
/// Synchronous SMTP email provider using lettre.
pub struct SmtpProvider {
    transport: lettre::transport::smtp::SmtpTransport,
    from: lettre::message::Mailbox,
}

#[cfg(feature = "smtp")]
impl SmtpProvider {
    /// Create a new SMTP provider with the given connection details.
    pub fn new(
        host: impl Into<String>,
        port: u16,
        username: Option<String>,
        password: Option<String>,
        from: lettre::message::Mailbox,
    ) -> Result<Self, EmailError> {
        use lettre::transport::smtp::authentication::Credentials;

        let mut builder =
            lettre::transport::smtp::SmtpTransport::builder_dangerous(host.into()).port(port);

        if let (Some(user), Some(pass)) = (username, password) {
            builder = builder.credentials(Credentials::new(user, pass));
        }

        let transport = builder.build();
        Ok(Self { transport, from })
    }
}

#[cfg(feature = "smtp")]
/// Build a lettre `Message` from an `EmailMessage`.
fn build_lettre_message(
    from: lettre::message::Mailbox,
    message: &EmailMessage,
) -> Result<lettre::Message, EmailError> {
    use lettre::message::header::ContentType;
    use lettre::{Message, message::Mailbox};

    let to: Mailbox = message
        .to
        .first()
        .and_then(|addr| addr.parse().ok())
        .ok_or_else(|| EmailError::provider("invalid recipient address"))?;

    let mut builder = Message::builder()
        .from(from)
        .to(to)
        .subject(&message.subject);

    for addr in &message.cc {
        if let Ok(mailbox) = addr.parse::<Mailbox>() {
            builder = builder.cc(mailbox);
        }
    }

    for addr in &message.bcc {
        if let Ok(mailbox) = addr.parse::<Mailbox>() {
            builder = builder.bcc(mailbox);
        }
    }

    let email = if let Some(html) = &message.html_body {
        builder.header(ContentType::TEXT_HTML).body(html.clone())
    } else if let Some(text) = &message.text_body {
        builder.header(ContentType::TEXT_PLAIN).body(text.clone())
    } else {
        builder.header(ContentType::TEXT_PLAIN).body(String::new())
    };

    email.map_err(|e| EmailError::provider(e.to_string()))
}

#[cfg(feature = "smtp")]
impl EmailProvider for SmtpProvider {
    fn send(
        &self,
        message: &EmailMessage,
    ) -> impl std::future::Future<Output = Result<(), EmailError>> + Send {
        let from = self.from.clone();
        let transport = self.transport.clone();
        let message = message.clone();

        async move {
            use lettre::Transport;

            let email = build_lettre_message(from, &message)?;

            transport
                .send(&email)
                .map_err(|e| EmailError::provider(e.to_string()))?;

            Ok(())
        }
    }
}

#[cfg(feature = "smtp")]
/// Asynchronous SMTP email provider using lettre's async transport.
pub struct AsyncSmtpProvider {
    transport: lettre::transport::smtp::AsyncSmtpTransport<lettre::Tokio1Executor>,
    from: lettre::message::Mailbox,
}

#[cfg(feature = "smtp")]
impl AsyncSmtpProvider {
    /// Create a new async SMTP provider with the given connection details.
    pub fn new(
        host: impl Into<String>,
        port: u16,
        username: Option<String>,
        password: Option<String>,
        from: lettre::message::Mailbox,
    ) -> Result<Self, EmailError> {
        use lettre::transport::smtp::authentication::Credentials;

        let mut builder =
            lettre::transport::smtp::AsyncSmtpTransport::<lettre::Tokio1Executor>::builder_dangerous(
                host.into(),
            )
            .port(port);

        if let (Some(user), Some(pass)) = (username, password) {
            builder = builder.credentials(Credentials::new(user, pass));
        }

        let transport = builder.build();
        Ok(Self { transport, from })
    }
}

#[cfg(feature = "smtp")]
impl EmailProvider for AsyncSmtpProvider {
    fn send(
        &self,
        message: &EmailMessage,
    ) -> impl std::future::Future<Output = Result<(), EmailError>> + Send {
        let from = self.from.clone();
        let transport = self.transport.clone();
        let message = message.clone();

        async move {
            use lettre::AsyncTransport;

            let email = build_lettre_message(from, &message)?;

            transport
                .send(email)
                .await
                .map_err(|e| EmailError::provider(e.to_string()))?;

            Ok(())
        }
    }
}
