use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// An email log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailLogEntry {
    /// Unique identifier for the log entry.
    pub id: String,
    /// Timestamp of the log entry.
    pub timestamp: DateTime<Utc>,
    /// Sender email address.
    pub from: String,
    /// Recipient email addresses.
    pub to: Vec<String>,
    /// Email subject.
    pub subject: String,
    /// Delivery status.
    pub status: LogStatus,
    /// Provider used to send the email.
    pub provider: String,
    /// Error message if the email failed.
    pub error: Option<String>,
}

/// Status of an email delivery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LogStatus {
    /// Email was sent successfully.
    Sent,
    /// Email failed to send.
    Failed,
    /// Email is queued for sending.
    Queued,
}

/// Trait for persisting email log entries.
pub trait AuditLogger: Send + Sync {
    /// Log an email entry.
    fn log(&self, entry: &EmailLogEntry) -> Result<(), crate::error::EmailError>;
}

/// Simple in-memory audit logger for testing.
pub struct InMemoryAuditLog {
    entries: std::sync::Mutex<Vec<EmailLogEntry>>,
}

impl InMemoryAuditLog {
    /// Create a new in-memory audit log.
    pub fn new() -> Self {
        Self {
            entries: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Get all logged entries.
    pub fn entries(&self) -> Vec<EmailLogEntry> {
        self.entries.lock().unwrap().clone()
    }
}

impl Default for InMemoryAuditLog {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditLogger for InMemoryAuditLog {
    fn log(&self, entry: &EmailLogEntry) -> Result<(), crate::error::EmailError> {
        self.entries.lock().unwrap().push(entry.clone());
        Ok(())
    }
}
