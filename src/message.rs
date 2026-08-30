use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// An email message ready to be sent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailMessage {
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    pub html_body: Option<String>,
    pub text_body: Option<String>,
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub filename: String,
    pub content_type: String,
    pub path: Option<PathBuf>,
    pub bytes: Option<Vec<u8>>,
}

impl EmailMessage {
    pub fn builder() -> EmailMessageBuilder {
        EmailMessageBuilder::default()
    }
}

#[derive(Debug, Default)]
pub struct EmailMessageBuilder {
    from: Option<String>,
    to: Vec<String>,
    subject: Option<String>,
    html_body: Option<String>,
    text_body: Option<String>,
    attachments: Vec<Attachment>,
}

impl EmailMessageBuilder {
    pub fn from(mut self, from: impl Into<String>) -> Self {
        self.from = Some(from.into());
        self
    }

    pub fn to(mut self, to: impl Into<String>) -> Self {
        self.to.push(to.into());
        self
    }

    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    pub fn html_body(mut self, html: impl Into<String>) -> Self {
        self.html_body = Some(html.into());
        self
    }

    pub fn text_body(mut self, text: impl Into<String>) -> Self {
        self.text_body = Some(text.into());
        self
    }

    pub fn attachment(mut self, att: Attachment) -> Self {
        self.attachments.push(att);
        self
    }

    pub fn build(self) -> Result<EmailMessage, crate::error::EmailError> {
        Ok(EmailMessage {
            from: self.from.ok_or_else(|| crate::error::EmailError::provider("from is required"))?,
            to: if self.to.is_empty() {
                return Err(crate::error::EmailError::provider("at least one recipient is required"));
            } else {
                self.to
            },
            subject: self
                .subject
                .unwrap_or_default(),
            html_body: self.html_body,
            text_body: self.text_body,
            attachments: self.attachments,
        })
    }
}
