//! Email provider abstraction.
//!
//! Two traits cooperate here:
//!
//! * [`EmailProvider`] — the original trait (0.2.x): `send` an
//!   [`EmailMessage`], fire-and-forget, `impl Future` based. Unchanged for
//!   backward compatibility; drives [`crate::EmailClient`].
//! * [`MailProvider`] — the richer trait (0.3.0): `send` returns a
//!   [`SendReceipt`] with the provider-assigned message id, and the trait is
//!   dyn-compatible, so heterogeneous providers compose as
//!   `Box<dyn MailProvider>`.
//!
//! Every built-in HTTP/SMTP provider implements **both** traits, and
//! `Box<dyn MailProvider>` implements `EmailProvider`, so either trait works
//! with [`crate::EmailClient`] and the queue:
//!
//! ```
//! use std::future::Future;
//! use std::pin::Pin;
//! use mailkit::provider::{MailProvider, SendReceipt};
//! use mailkit::{EmailClient, EmailError, EmailMessage};
//!
//! struct LoggingProvider;
//!
//! impl MailProvider for LoggingProvider {
//!     fn send<'a>(&'a self, message: &'a EmailMessage)
//!         -> Pin<Box<dyn Future<Output = Result<SendReceipt, EmailError>> + Send + 'a>>
//!     {
//!         Box::pin(async {
//!             Ok(SendReceipt::new("logging", Some("id-1".into())))
//!         })
//!     }
//!     fn name(&self) -> &str { "logging" }
//! }
//!
//! // Works with the unified client, with receipts:
//! let client = EmailClient::new(Box::new(LoggingProvider) as Box<dyn MailProvider>);
//! ```

use std::future::Future;
use std::pin::Pin;

#[cfg(feature = "postmark")]
mod postmark;
#[cfg(feature = "resend")]
mod resend;
#[cfg(feature = "sendgrid")]
mod sendgrid;
#[cfg(feature = "ses")]
mod ses;
#[cfg(feature = "smtp")]
mod smtp;

use crate::error::EmailError;
use crate::message::EmailMessage;

#[cfg(feature = "postmark")]
pub use postmark::PostmarkProvider;
#[cfg(feature = "resend")]
pub use resend::ResendProvider;
#[cfg(feature = "sendgrid")]
pub use sendgrid::SendGridProvider;
#[cfg(feature = "ses")]
pub use ses::SesProvider;
#[cfg(feature = "smtp")]
pub use smtp::{AsyncSmtpProvider, SmtpProvider};

/// Trait for email transport providers (0.2.x API, unchanged).
///
/// For receipts and dyn-compatible usage, prefer [`MailProvider`].
pub trait EmailProvider: Send + Sync {
    /// Send an email message.
    fn send(
        &self,
        message: &EmailMessage,
    ) -> impl std::future::Future<Output = Result<(), EmailError>> + Send;
}

/// Receipt returned by a provider after it accepts a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendReceipt {
    /// Provider that accepted the message (e.g. `"ses"`).
    pub provider: String,
    /// Provider-assigned message identifier, when reported.
    pub message_id: Option<String>,
}

impl SendReceipt {
    /// Create a receipt.
    #[must_use]
    pub fn new(provider: impl Into<String>, message_id: Option<String>) -> Self {
        Self {
            provider: provider.into(),
            message_id,
        }
    }
}

/// Dyn-compatible provider trait returning a [`SendReceipt`].
///
/// Implemented by every built-in provider; implement it to add custom
/// transports (e.g. a test double or an internal gateway).
pub trait MailProvider: Send + Sync {
    /// Send an email message, returning the provider's receipt.
    fn send<'a>(
        &'a self,
        message: &'a EmailMessage,
    ) -> Pin<Box<dyn Future<Output = Result<SendReceipt, EmailError>> + Send + 'a>>;

    /// Short provider name (used in receipts and audit logs).
    fn name(&self) -> &str;
}

impl MailProvider for Box<dyn MailProvider> {
    fn send<'a>(
        &'a self,
        message: &'a EmailMessage,
    ) -> Pin<Box<dyn Future<Output = Result<SendReceipt, EmailError>> + Send + 'a>> {
        self.as_ref().send(message)
    }

    fn name(&self) -> &str {
        self.as_ref().name()
    }
}

impl EmailProvider for Box<dyn MailProvider> {
    fn send(
        &self,
        message: &EmailMessage,
    ) -> impl std::future::Future<Output = Result<(), EmailError>> + Send {
        let inner = self.as_ref();
        async move { inner.send(message).await.map(|_| ()) }
    }
}

/// Discard a receipt where the 0.2.x [`EmailProvider`] API only reports
/// success/failure. Used by the built-in providers' `EmailProvider` impls.
#[cfg(any(
    feature = "resend",
    feature = "smtp",
    feature = "ses",
    feature = "sendgrid",
    feature = "postmark"
))]
macro_rules! impl_email_provider_via_mail {
    ($ty:ty) => {
        impl EmailProvider for $ty {
            fn send(
                &self,
                message: &EmailMessage,
            ) -> impl std::future::Future<Output = Result<(), EmailError>> + Send {
                let this = self;
                async move { MailProvider::send(this, message).await.map(|_| ()) }
            }
        }
    };
}

#[cfg(feature = "resend")]
impl_email_provider_via_mail!(ResendProvider);
#[cfg(feature = "ses")]
impl_email_provider_via_mail!(SesProvider);
#[cfg(feature = "sendgrid")]
impl_email_provider_via_mail!(SendGridProvider);
#[cfg(feature = "postmark")]
impl_email_provider_via_mail!(PostmarkProvider);
#[cfg(feature = "smtp")]
impl_email_provider_via_mail!(SmtpProvider);
#[cfg(feature = "smtp")]
impl_email_provider_via_mail!(AsyncSmtpProvider);
