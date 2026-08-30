use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailLogEntry {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    pub status: LogStatus,
    pub provider: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LogStatus {
    Sent,
    Failed,
    Queued,
}

/// Trait for persisting email log entries.
pub trait AuditLogger: Send + Sync {
    fn log(&self, entry: &EmailLogEntry) -> Result<(), crate::error::EmailError>;
}

/// Simple in-memory audit logger for testing.
pub struct InMemoryAuditLog {
    entries: std::sync::Mutex<Vec<EmailLogEntry>>,
}

impl InMemoryAuditLog {
    pub fn new() -> Self {
        Self {
            entries: std::sync::Mutex::new(Vec::new()),
        }
    }

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
