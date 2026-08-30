use std::collections::VecDeque;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::error::EmailError;
use crate::message::EmailMessage;
use crate::provider::EmailProvider;

struct QueuedEmail {
    message: EmailMessage,
    attempts: u32,
    max_retries: u32,
}

/// Async email queue with retry logic.
pub struct EmailQueue {
    inner: Arc<Mutex<VecDeque<QueuedEmail>>>,
}

impl EmailQueue {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub async fn enqueue(&self, message: EmailMessage) {
        let mut guard = self.inner.lock().await;
        guard.push_back(QueuedEmail {
            message,
            attempts: 0,
            max_retries: 3,
        });
    }

    pub async fn process<P: EmailProvider>(
        &self,
        provider: &P,
    ) -> Result<(), EmailError> {
        let mut guard = self.inner.lock().await;
        let mut failed = Vec::new();

        while let Some(mut item) = guard.pop_front() {
            match provider.send(&item.message).await {
                Ok(()) => {}
                Err(e) => {
                    item.attempts += 1;
                    if item.attempts < item.max_retries {
                        failed.push(item);
                    } else {
                        eprintln!(
                            "email to {:?} failed after {} attempts: {e}",
                            item.message.to, item.attempts
                        );
                    }
                }
            }
        }

        for item in failed {
            guard.push_back(item);
        }

        Ok(())
    }

    pub async fn len(&self) -> usize {
        self.inner.lock().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.inner.lock().await.is_empty()
    }
}
