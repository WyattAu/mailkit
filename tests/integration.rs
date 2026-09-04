//! Integration tests for the mailkit crate.
//!
//! Tests EmailMessage builder, EmailLogEntry creation, InMemoryAuditLog
//! append/query, and EmailError display.

use std::path::PathBuf;

use mailkit::audit::{AuditLogger, EmailLogEntry, InMemoryAuditLog, LogStatus};
use mailkit::error::EmailError;
use mailkit::message::{Attachment, EmailMessage};

// ---------------------------------------------------------------------------
// EmailMessage builder
// ---------------------------------------------------------------------------

#[test]
fn builder_to_from_subject_body() {
    let msg = EmailMessage::builder()
        .from("sender@example.com")
        .to("recipient@example.com")
        .subject("Hello World")
        .text_body("Plain text body")
        .html_body("<p>HTML body</p>")
        .build()
        .unwrap();

    assert_eq!(msg.from, "sender@example.com");
    assert_eq!(msg.to, vec!["recipient@example.com"]);
    assert_eq!(msg.subject, "Hello World");
    assert_eq!(msg.text_body.as_deref(), Some("Plain text body"));
    assert_eq!(msg.html_body.as_deref(), Some("<p>HTML body</p>"));
    assert!(msg.cc.is_empty());
    assert!(msg.bcc.is_empty());
    assert!(msg.attachments.is_empty());
}

#[test]
fn builder_multiple_to_recipients() {
    let msg = EmailMessage::builder()
        .from("a@b.com")
        .to("c@d.com")
        .to("e@f.com")
        .to("g@h.com")
        .subject("Multi")
        .build()
        .unwrap();

    assert_eq!(msg.to.len(), 3);
    assert_eq!(msg.to, vec!["c@d.com", "e@f.com", "g@h.com"]);
}

#[test]
fn builder_cc_and_bcc() {
    let msg = EmailMessage::builder()
        .from("sender@example.com")
        .to("to@example.com")
        .cc("cc1@example.com")
        .cc("cc2@example.com")
        .bcc("bcc1@example.com")
        .subject("CC and BCC test")
        .build()
        .unwrap();

    assert_eq!(msg.to, vec!["to@example.com"]);
    assert_eq!(msg.cc, vec!["cc1@example.com", "cc2@example.com"]);
    assert_eq!(msg.bcc, vec!["bcc1@example.com"]);
}

#[test]
fn builder_attachment_from_path() {
    let att = Attachment {
        filename: "report.pdf".into(),
        content_type: "application/pdf".into(),
        path: Some(PathBuf::from("/tmp/report.pdf")),
        bytes: None,
    };
    let msg = EmailMessage::builder()
        .from("a@b.com")
        .to("c@d.com")
        .attachment(att)
        .subject("With file")
        .build()
        .unwrap();

    assert_eq!(msg.attachments.len(), 1);
    assert_eq!(msg.attachments[0].filename, "report.pdf");
    assert_eq!(msg.attachments[0].content_type, "application/pdf");
    assert_eq!(
        msg.attachments[0].path,
        Some(PathBuf::from("/tmp/report.pdf"))
    );
}

#[test]
fn builder_attachment_from_bytes() {
    let att = Attachment {
        filename: "image.png".into(),
        content_type: "image/png".into(),
        path: None,
        bytes: Some(vec![0x89, 0x50, 0x4E, 0x47]),
    };
    let msg = EmailMessage::builder()
        .from("a@b.com")
        .to("c@d.com")
        .attachment(att)
        .subject("With bytes")
        .build()
        .unwrap();

    assert_eq!(msg.attachments[0].bytes, Some(vec![0x89, 0x50, 0x4E, 0x47]));
}

#[test]
fn builder_multiple_attachments() {
    let a1 = Attachment {
        filename: "a.txt".into(),
        content_type: "text/plain".into(),
        path: None,
        bytes: Some(b"hello".to_vec()),
    };
    let a2 = Attachment {
        filename: "b.bin".into(),
        content_type: "application/octet-stream".into(),
        path: None,
        bytes: Some(vec![0, 1, 2]),
    };

    let msg = EmailMessage::builder()
        .from("a@b.com")
        .to("c@d.com")
        .subject("Multi att")
        .attachment(a1)
        .attachment(a2)
        .build()
        .unwrap();

    assert_eq!(msg.attachments.len(), 2);
    assert_eq!(msg.attachments[0].filename, "a.txt");
    assert_eq!(msg.attachments[1].filename, "b.bin");
}

#[test]
fn builder_missing_from_returns_error() {
    let result = EmailMessage::builder()
        .to("x@y.com")
        .subject("No from")
        .build();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("from is required"));
}

#[test]
fn builder_missing_to_returns_error() {
    let result = EmailMessage::builder()
        .from("a@b.com")
        .subject("No to")
        .build();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("at least one recipient"));
}

#[test]
fn builder_subject_defaults_to_empty_string() {
    let msg = EmailMessage::builder()
        .from("a@b.com")
        .to("c@d.com")
        .build()
        .unwrap();
    assert_eq!(msg.subject, "");
}

#[test]
fn builder_body_defaults_to_none() {
    let msg = EmailMessage::builder()
        .from("a@b.com")
        .to("c@d.com")
        .build()
        .unwrap();
    assert!(msg.text_body.is_none());
    assert!(msg.html_body.is_none());
}

// ---------------------------------------------------------------------------
// EmailLogEntry creation
// ---------------------------------------------------------------------------

#[test]
fn log_entry_sent_status() {
    let entry = EmailLogEntry {
        id: "log-001".into(),
        timestamp: chrono::Utc::now(),
        from: "a@b.com".into(),
        to: vec!["c@d.com".into()],
        subject: "Test Email".into(),
        status: LogStatus::Sent,
        provider: "resend".into(),
        error: None,
    };

    assert_eq!(entry.id, "log-001");
    assert_eq!(entry.status, LogStatus::Sent);
    assert!(entry.error.is_none());
    assert_eq!(entry.provider, "resend");
}

#[test]
fn log_entry_failed_status_with_error() {
    let entry = EmailLogEntry {
        id: "log-002".into(),
        timestamp: chrono::Utc::now(),
        from: "a@b.com".into(),
        to: vec![],
        subject: "Failed".into(),
        status: LogStatus::Failed,
        provider: "smtp".into(),
        error: Some("connection timeout".into()),
    };

    assert_eq!(entry.status, LogStatus::Failed);
    assert_eq!(entry.error.as_deref(), Some("connection timeout"));
}

#[test]
fn log_entry_queued_status() {
    let entry = EmailLogEntry {
        id: "log-003".into(),
        timestamp: chrono::Utc::now(),
        from: "a@b.com".into(),
        to: vec!["c@d.com".into()],
        subject: "Queued".into(),
        status: LogStatus::Queued,
        provider: "resend".into(),
        error: None,
    };

    assert_eq!(entry.status, LogStatus::Queued);
}

#[test]
fn log_status_equality() {
    assert_eq!(LogStatus::Sent, LogStatus::Sent);
    assert_eq!(LogStatus::Failed, LogStatus::Failed);
    assert_eq!(LogStatus::Queued, LogStatus::Queued);
    assert_ne!(LogStatus::Sent, LogStatus::Failed);
    assert_ne!(LogStatus::Sent, LogStatus::Queued);
    assert_ne!(LogStatus::Failed, LogStatus::Queued);
}

// ---------------------------------------------------------------------------
// InMemoryAuditLog append/query
// ---------------------------------------------------------------------------

#[test]
fn audit_log_new_is_empty() {
    let log = InMemoryAuditLog::new();
    assert!(log.entries().is_empty());
}

#[test]
fn audit_log_default_is_empty() {
    let log = InMemoryAuditLog::default();
    assert!(log.entries().is_empty());
}

#[test]
fn audit_log_append_single_entry() {
    let log = InMemoryAuditLog::new();
    let entry = EmailLogEntry {
        id: "1".into(),
        timestamp: chrono::Utc::now(),
        from: "a@b.com".into(),
        to: vec!["c@d.com".into()],
        subject: "First".into(),
        status: LogStatus::Sent,
        provider: "resend".into(),
        error: None,
    };

    log.log(&entry).unwrap();
    let entries = log.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, "1");
    assert_eq!(entries[0].subject, "First");
}

#[test]
fn audit_log_append_multiple_entries() {
    let log = InMemoryAuditLog::new();

    for i in 0..10 {
        let entry = EmailLogEntry {
            id: format!("id-{i}"),
            timestamp: chrono::Utc::now(),
            from: "sender@test.com".into(),
            to: vec!["recipient@test.com".into()],
            subject: format!("Subject {i}"),
            status: if i % 2 == 0 {
                LogStatus::Sent
            } else {
                LogStatus::Failed
            },
            provider: "resend".into(),
            error: if i % 2 == 0 {
                None
            } else {
                Some(format!("error {i}"))
            },
        };
        log.log(&entry).unwrap();
    }

    let entries = log.entries();
    assert_eq!(entries.len(), 10);

    // Check that entries are in order
    assert_eq!(entries[0].id, "id-0");
    assert_eq!(entries[9].id, "id-9");

    // Check status alternation
    assert_eq!(entries[0].status, LogStatus::Sent);
    assert_eq!(entries[1].status, LogStatus::Failed);
}

#[test]
fn audit_log_entries_returns_clones() {
    let log = InMemoryAuditLog::new();
    let entry = EmailLogEntry {
        id: "1".into(),
        timestamp: chrono::Utc::now(),
        from: "a@b.com".into(),
        to: vec!["c@d.com".into()],
        subject: "Clone test".into(),
        status: LogStatus::Sent,
        provider: "resend".into(),
        error: None,
    };

    log.log(&entry).unwrap();
    let e1 = log.entries();
    let e2 = log.entries();
    assert_eq!(e1.len(), e2.len());
    assert_eq!(e1[0].id, e2[0].id);
}

// ---------------------------------------------------------------------------
// EmailError display
// ---------------------------------------------------------------------------

#[test]
fn error_provider_display() {
    let err = EmailError::provider("connection failed");
    assert_eq!(err.to_string(), "provider error: connection failed");
}

#[test]
fn error_template_display() {
    let err = EmailError::template("undefined variable");
    assert_eq!(err.to_string(), "template error: undefined variable");
}

#[test]
fn error_serialization_display() {
    let json_err = serde_json::from_str::<serde_json::Value>("{bad").unwrap_err();
    let err = EmailError::Serialization(json_err);
    let msg = err.to_string();
    assert!(msg.starts_with("serialization error:"));
}

#[test]
fn error_io_display() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing file");
    let err = EmailError::Io(io_err);
    assert_eq!(err.to_string(), "io error: missing file");
}

#[test]
fn error_debug_format() {
    let err = EmailError::provider("test");
    let debug = format!("{:?}", err);
    assert!(debug.contains("Provider"));
}

#[test]
fn error_variants_are_distinct() {
    let e1 = EmailError::provider("a");
    let e2 = EmailError::template("b");
    let e3 = EmailError::Serialization(
        serde_json::from_str::<serde_json::Value>("not json!!").unwrap_err(),
    );
    let e4 = EmailError::Io(std::io::Error::other("x"));

    assert!(e1.to_string() != e2.to_string());
    assert!(e2.to_string() != e3.to_string());
    assert!(e3.to_string() != e4.to_string());
}
