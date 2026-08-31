use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// An email message ready to be sent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailMessage {
    /// Sender email address.
    pub from: String,
    /// Recipient email addresses.
    pub to: Vec<String>,
    /// Carbon copy recipients.
    pub cc: Vec<String>,
    /// Blind carbon copy recipients.
    pub bcc: Vec<String>,
    /// Email subject.
    pub subject: String,
    /// HTML body content.
    pub html_body: Option<String>,
    /// Plain text body content.
    pub text_body: Option<String>,
    /// File attachments.
    pub attachments: Vec<Attachment>,
}

/// A file attachment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    /// Filename.
    pub filename: String,
    /// MIME content type.
    pub content_type: String,
    /// File path (for file-based attachments).
    pub path: Option<PathBuf>,
    /// Raw bytes (for inline attachments).
    pub bytes: Option<Vec<u8>>,
}

impl EmailMessage {
    /// Create a new email message builder.
    pub fn builder() -> EmailMessageBuilder {
        EmailMessageBuilder::default()
    }
}

/// Builder for constructing an [`EmailMessage`].
#[derive(Debug, Default)]
pub struct EmailMessageBuilder {
    from: Option<String>,
    to: Vec<String>,
    cc: Vec<String>,
    bcc: Vec<String>,
    subject: Option<String>,
    html_body: Option<String>,
    text_body: Option<String>,
    attachments: Vec<Attachment>,
}

impl EmailMessageBuilder {
    /// Set the sender email address.
    pub fn from(mut self, from: impl Into<String>) -> Self {
        self.from = Some(from.into());
        self
    }

    /// Add a recipient email address.
    pub fn to(mut self, to: impl Into<String>) -> Self {
        self.to.push(to.into());
        self
    }

    /// Add a carbon copy recipient.
    pub fn cc(mut self, cc: impl Into<String>) -> Self {
        self.cc.push(cc.into());
        self
    }

    /// Add a blind carbon copy recipient.
    pub fn bcc(mut self, bcc: impl Into<String>) -> Self {
        self.bcc.push(bcc.into());
        self
    }

    /// Set the email subject.
    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    /// Set the HTML body content.
    pub fn html_body(mut self, html: impl Into<String>) -> Self {
        self.html_body = Some(html.into());
        self
    }

    /// Set the plain text body content.
    pub fn text_body(mut self, text: impl Into<String>) -> Self {
        self.text_body = Some(text.into());
        self
    }

    /// Add a file attachment.
    pub fn attachment(mut self, att: Attachment) -> Self {
        self.attachments.push(att);
        self
    }

    /// Build the email message.
    pub fn build(self) -> Result<EmailMessage, crate::error::EmailError> {
        Ok(EmailMessage {
            from: self.from.ok_or_else(|| crate::error::EmailError::provider("from is required"))?,
            to: if self.to.is_empty() {
                return Err(crate::error::EmailError::provider("at least one recipient is required"));
            } else {
                self.to
            },
            cc: self.cc,
            bcc: self.bcc,
            subject: self
                .subject
                .unwrap_or_default(),
            html_body: self.html_body,
            text_body: self.text_body,
            attachments: self.attachments,
        })
    }
}
