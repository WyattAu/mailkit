/// Errors that can occur in email operations.
#[derive(Debug, thiserror::Error)]
pub enum EmailError {
    /// Provider error.
    #[error("provider error: {0}")]
    Provider(String),

    /// Template error.
    #[error("template error: {0}")]
    Template(String),

    /// Serialization error.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// I/O error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl EmailError {
    /// Create a provider error.
    pub fn provider(msg: impl Into<String>) -> Self {
        Self::Provider(msg.into())
    }

    /// Create a template error.
    pub fn template(msg: impl Into<String>) -> Self {
        Self::Template(msg.into())
    }
}
