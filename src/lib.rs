pub mod audit;
pub mod error;
pub mod message;
pub mod provider;
pub mod queue;

pub use error::EmailError;
pub use message::EmailMessage;
pub use provider::{EmailProvider, ResendProvider};
pub use queue::EmailQueue;

#[cfg(feature = "smtp")]
pub use provider::SmtpProvider;

/// Unified email client backed by a configurable provider.
pub struct EmailClient<P: EmailProvider> {
    provider: P,
    queue: EmailQueue,
}

impl<P: EmailProvider> EmailClient<P> {
    pub fn new(provider: P) -> Self {
        Self {
            provider,
            queue: EmailQueue::new(),
        }
    }

    pub async fn send(&self, message: EmailMessage) -> Result<(), EmailError> {
        self.provider.send(&message).await
    }

    pub fn queue(&self) -> &EmailQueue {
        &self.queue
    }
}
