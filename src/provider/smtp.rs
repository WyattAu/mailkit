//! SMTP providers (`smtp` feature, via lettre).

use std::future::Future;
use std::pin::Pin;

use crate::error::EmailError;
use crate::message::EmailMessage;

use super::{MailProvider, SendReceipt};

/// Synchronous SMTP email provider using lettre.
#[derive(Clone)]
pub struct SmtpProvider {
    transport: lettre::transport::smtp::SmtpTransport,
    from: lettre::message::Mailbox,
}

impl SmtpProvider {
    /// Create a new SMTP provider with the given connection details.
    ///
    /// # Errors
    /// Returns an error if the recipient parsing fails at send time rather
    /// than construction; construction itself always succeeds.
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

/// Build a lettre `Message` from an [`EmailMessage`].
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

impl MailProvider for SmtpProvider {
    fn send<'a>(
        &'a self,
        message: &'a EmailMessage,
    ) -> Pin<Box<dyn Future<Output = Result<SendReceipt, EmailError>> + Send + 'a>> {
        Box::pin(async move {
            use lettre::Transport;

            let email = build_lettre_message(self.from.clone(), message)?;
            self.transport
                .send(&email)
                .map_err(|e| EmailError::provider(e.to_string()))?;
            Ok(SendReceipt::new("smtp", None))
        })
    }

    fn name(&self) -> &str {
        "smtp"
    }
}

/// Asynchronous SMTP email provider using lettre's async transport.
#[derive(Clone)]
pub struct AsyncSmtpProvider {
    transport: lettre::transport::smtp::AsyncSmtpTransport<lettre::Tokio1Executor>,
    from: lettre::message::Mailbox,
}

impl AsyncSmtpProvider {
    /// Create a new async SMTP provider with the given connection details.
    ///
    /// # Errors
    /// Construction always succeeds; failures surface at send time.
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

impl MailProvider for AsyncSmtpProvider {
    fn send<'a>(
        &'a self,
        message: &'a EmailMessage,
    ) -> Pin<Box<dyn Future<Output = Result<SendReceipt, EmailError>> + Send + 'a>> {
        Box::pin(async move {
            use lettre::AsyncTransport;

            let email = build_lettre_message(self.from.clone(), message)?;
            self.transport
                .send(email)
                .await
                .map_err(|e| EmailError::provider(e.to_string()))?;
            Ok(SendReceipt::new("smtp", None))
        })
    }

    fn name(&self) -> &str {
        "smtp"
    }
}
