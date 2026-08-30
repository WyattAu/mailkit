#[derive(Debug, thiserror::Error)]
pub enum EmailError {
    #[error("provider error: {0}")]
    Provider(String),

    #[error("template error: {0}")]
    Template(String),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl EmailError {
    pub fn provider(msg: impl Into<String>) -> Self {
        Self::Provider(msg.into())
    }

    pub fn template(msg: impl Into<String>) -> Self {
        Self::Template(msg.into())
    }
}
