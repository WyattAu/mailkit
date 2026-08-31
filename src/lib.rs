#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Email client library for Rust with pluggable providers and audit logging.

/// Audit logging for email operations.
pub mod audit;
/// Error types.
pub mod error;
/// Email message types.
pub mod message;
/// Email provider implementations.
pub mod provider;
/// Email queue for deferred sending.
pub mod queue;

pub use error::EmailError;
pub use message::EmailMessage;
pub use provider::{EmailProvider, ResendProvider};
pub use queue::EmailQueue;

#[cfg(feature = "smtp")]
pub use provider::{AsyncSmtpProvider, SmtpProvider};

/// Unified email client backed by a configurable provider.
pub struct EmailClient<P: EmailProvider> {
    provider: P,
    queue: EmailQueue,
}

impl<P: EmailProvider> EmailClient<P> {
    /// Create a new email client with the given provider.
    pub fn new(provider: P) -> Self {
        Self {
            provider,
            queue: EmailQueue::new(),
        }
    }

    /// Send an email message.
    pub async fn send(&self, message: EmailMessage) -> Result<(), EmailError> {
        self.provider.send(&message).await
    }

    /// Get a reference to the email queue.
    pub fn queue(&self) -> &EmailQueue {
        &self.queue
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::{AuditLogger, EmailLogEntry, InMemoryAuditLog, LogStatus};
    use crate::message::Attachment;
    use chrono::Utc;
    use std::path::PathBuf;

    // ---- EmailMessage builder tests ----

    #[test]
    fn builder_basic_message() {
        let msg = EmailMessage::builder()
            .from("sender@example.com")
            .to("recipient@example.com")
            .subject("Hello")
            .text_body("Hi there")
            .html_body("<p>Hi there</p>")
            .build()
            .unwrap();

        assert_eq!(msg.from, "sender@example.com");
        assert_eq!(msg.to, vec!["recipient@example.com"]);
        assert!(msg.cc.is_empty());
        assert!(msg.bcc.is_empty());
        assert_eq!(msg.subject, "Hello");
        assert_eq!(msg.text_body.as_deref(), Some("Hi there"));
        assert_eq!(msg.html_body.as_deref(), Some("<p>Hi there</p>"));
        assert!(msg.attachments.is_empty());
    }

    #[test]
    fn builder_multiple_recipients() {
        let msg = EmailMessage::builder()
            .from("a@b.com")
            .to("c@d.com")
            .to("e@f.com")
            .subject("Multi")
            .build()
            .unwrap();

        assert_eq!(msg.to.len(), 2);
        assert_eq!(msg.to, vec!["c@d.com", "e@f.com"]);
    }

    #[test]
    fn builder_with_cc_and_bcc() {
        let msg = EmailMessage::builder()
            .from("a@b.com")
            .to("c@d.com")
            .cc("cc1@example.com")
            .cc("cc2@example.com")
            .bcc("bcc@example.com")
            .subject("CC and BCC")
            .build()
            .unwrap();

        assert_eq!(msg.to, vec!["c@d.com"]);
        assert_eq!(msg.cc, vec!["cc1@example.com", "cc2@example.com"]);
        assert_eq!(msg.bcc, vec!["bcc@example.com"]);
    }

    #[test]
    fn builder_missing_from_fails() {
        let result = EmailMessage::builder()
            .to("x@y.com")
            .subject("No sender")
            .build();

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("from is required"));
    }

    #[test]
    fn builder_missing_recipient_fails() {
        let result = EmailMessage::builder()
            .from("a@b.com")
            .subject("No recipient")
            .build();

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("at least one recipient"));
    }

    #[test]
    fn builder_with_attachment() {
        let att = Attachment {
            filename: "doc.pdf".into(),
            content_type: "application/pdf".into(),
            path: Some(PathBuf::from("/tmp/doc.pdf")),
            bytes: None,
        };
        let msg = EmailMessage::builder()
            .from("a@b.com")
            .to("c@d.com")
            .subject("With attachment")
            .attachment(att)
            .build()
            .unwrap();

        assert_eq!(msg.attachments.len(), 1);
        assert_eq!(msg.attachments[0].filename, "doc.pdf");
        assert_eq!(msg.attachments[0].content_type, "application/pdf");
    }

    #[test]
    fn builder_with_bytes_attachment() {
        let att = Attachment {
            filename: "data.bin".into(),
            content_type: "application/octet-stream".into(),
            path: None,
            bytes: Some(vec![0u8, 1, 2, 3]),
        };
        let msg = EmailMessage::builder()
            .from("a@b.com")
            .to("c@d.com")
            .subject("Bytes")
            .attachment(att)
            .build()
            .unwrap();

        assert_eq!(msg.attachments[0].bytes.as_ref().unwrap(), &[0, 1, 2, 3]);
    }

    #[test]
    fn builder_subject_defaults_to_empty() {
        let msg = EmailMessage::builder()
            .from("a@b.com")
            .to("c@d.com")
            .build()
            .unwrap();

        assert_eq!(msg.subject, "");
    }

    // ---- EmailLogEntry tests ----

    #[test]
    fn email_log_entry_creation() {
        let entry = EmailLogEntry {
            id: "log-1".into(),
            timestamp: Utc::now(),
            from: "a@b.com".into(),
            to: vec!["c@d.com".into()],
            subject: "Test".into(),
            status: LogStatus::Sent,
            provider: "resend".into(),
            error: None,
        };

        assert_eq!(entry.id, "log-1");
        assert_eq!(entry.status, LogStatus::Sent);
        assert!(entry.error.is_none());
    }

    #[test]
    fn email_log_entry_failed_status() {
        let entry = EmailLogEntry {
            id: "log-2".into(),
            timestamp: Utc::now(),
            from: "a@b.com".into(),
            to: vec![],
            subject: "Fail".into(),
            status: LogStatus::Failed,
            provider: "smtp".into(),
            error: Some("connection refused".into()),
        };

        assert_eq!(entry.status, LogStatus::Failed);
        assert_eq!(entry.error.as_deref(), Some("connection refused"));
    }

    // ---- InMemoryAuditLog tests ----

    #[test]
    fn in_memory_audit_log_append_and_query() {
        let log = InMemoryAuditLog::new();
        assert!(log.entries().is_empty());

        let entry = EmailLogEntry {
            id: "1".into(),
            timestamp: Utc::now(),
            from: "a@b.com".into(),
            to: vec!["c@d.com".into()],
            subject: "Subj".into(),
            status: LogStatus::Queued,
            provider: "resend".into(),
            error: None,
        };

        log.log(&entry).unwrap();
        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "1");
        assert_eq!(entries[0].status, LogStatus::Queued);
    }

    #[test]
    fn in_memory_audit_log_multiple_entries() {
        let log = InMemoryAuditLog::default();
        for i in 0..5 {
            let entry = EmailLogEntry {
                id: format!("id-{i}"),
                timestamp: Utc::now(),
                from: "a@b.com".into(),
                to: vec![],
                subject: format!("Subject {i}"),
                status: LogStatus::Sent,
                provider: "test".into(),
                error: None,
            };
            log.log(&entry).unwrap();
        }
        assert_eq!(log.entries().len(), 5);
    }

    // ---- EmailError display tests ----

    #[test]
    fn error_provider_display() {
        let err = EmailError::provider("timeout");
        assert_eq!(err.to_string(), "provider error: timeout");
    }

    #[test]
    fn error_template_display() {
        let err = EmailError::template("missing var");
        assert_eq!(err.to_string(), "template error: missing var");
    }

    #[test]
    fn error_serialization_display() {
        let json_err = serde_json::from_str::<serde_json::Value>("{bad").unwrap_err();
        let err = EmailError::Serialization(json_err);
        assert!(err.to_string().contains("serialization error:"));
    }

    #[test]
    fn error_io_display() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err = EmailError::Io(io_err);
        assert_eq!(err.to_string(), "io error: file missing");
    }

    // ---- EmailQueue tests ----

    #[tokio::test]
    async fn queue_enqueue_and_len() {
        let queue = EmailQueue::new();
        assert!(queue.is_empty().await);

        let msg = EmailMessage::builder()
            .from("a@b.com")
            .to("c@d.com")
            .subject("Q")
            .build()
            .unwrap();

        queue.enqueue(msg).await;
        assert_eq!(queue.len().await, 1);
        assert!(!queue.is_empty().await);
    }

    struct MockProvider;

    impl EmailProvider for MockProvider {
        async fn send(&self, _message: &EmailMessage) -> Result<(), EmailError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn queue_process_drains_queue() {
        let queue = EmailQueue::new();
        let msg = EmailMessage::builder()
            .from("a@b.com")
            .to("c@d.com")
            .subject("Drain")
            .build()
            .unwrap();

        queue.enqueue(msg).await;
        assert_eq!(queue.len().await, 1);

        let provider = MockProvider;
        queue.process(&provider).await.unwrap();
        assert!(queue.is_empty().await);
    }
}
