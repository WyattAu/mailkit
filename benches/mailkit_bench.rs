use criterion::{criterion_group, criterion_main, Criterion};
use mailkit::EmailMessage;
use mailkit::audit::{AuditLogger, EmailLogEntry, InMemoryAuditLog, LogStatus};
use mailkit::message::Attachment;
use chrono::Utc;
use std::path::PathBuf;

fn bench_email_builder_basic(c: &mut Criterion) {
    c.bench_function("email_builder_basic", |b| {
        b.iter(|| {
            EmailMessage::builder()
                .from("sender@example.com")
                .to("recipient@example.com")
                .subject("Hello")
                .text_body("Hi there")
                .html_body("<p>Hi there</p>")
                .build()
                .unwrap()
        });
    });
}

fn bench_email_builder_multiple_recipients(c: &mut Criterion) {
    c.bench_function("email_builder_multiple_recipients", |b| {
        b.iter(|| {
            EmailMessage::builder()
                .from("a@b.com")
                .to("c@d.com")
                .to("e@f.com")
                .to("g@h.com")
                .subject("Multi")
                .build()
                .unwrap()
        });
    });
}

fn bench_email_builder_with_cc_and_bcc(c: &mut Criterion) {
    c.bench_function("email_builder_with_cc_bcc", |b| {
        b.iter(|| {
            EmailMessage::builder()
                .from("a@b.com")
                .to("c@d.com")
                .cc("cc1@example.com")
                .cc("cc2@example.com")
                .bcc("bcc@example.com")
                .subject("CC and BCC")
                .build()
                .unwrap()
        });
    });
}

fn bench_email_builder_with_attachment(c: &mut Criterion) {
    let att = Attachment {
        filename: "doc.pdf".into(),
        content_type: "application/pdf".into(),
        path: Some(PathBuf::from("/tmp/doc.pdf")),
        bytes: None,
    };
    c.bench_function("email_builder_with_attachment", |b| {
        b.iter(|| {
            EmailMessage::builder()
                .from("a@b.com")
                .to("c@d.com")
                .subject("With attachment")
                .attachment(att.clone())
                .build()
                .unwrap()
        });
    });
}

fn bench_email_builder_with_bytes_attachment(c: &mut Criterion) {
    let att = Attachment {
        filename: "data.bin".into(),
        content_type: "application/octet-stream".into(),
        path: None,
        bytes: Some(vec![0u8; 1024]),
    };
    c.bench_function("email_builder_with_bytes_attachment", |b| {
        b.iter(|| {
            EmailMessage::builder()
                .from("a@b.com")
                .to("c@d.com")
                .subject("Bytes")
                .attachment(att.clone())
                .build()
                .unwrap()
        });
    });
}

fn bench_email_log_entry_creation(c: &mut Criterion) {
    c.bench_function("email_log_entry_creation", |b| {
        b.iter(|| {
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
            std::hint::black_box(entry);
        });
    });
}

fn bench_in_memory_audit_log_append(c: &mut Criterion) {
    let log = InMemoryAuditLog::new();
    c.bench_function("in_memory_audit_log_append", |b| {
        b.iter(|| {
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
            log.log(&entry).unwrap();
        });
    });
}

fn bench_in_memory_audit_log_entries(c: &mut Criterion) {
    let log = InMemoryAuditLog::new();
    for i in 0..100 {
        let entry = EmailLogEntry {
            id: format!("log-{i}"),
            timestamp: Utc::now(),
            from: "a@b.com".into(),
            to: vec!["c@d.com".into()],
            subject: format!("Subject {i}"),
            status: LogStatus::Sent,
            provider: "resend".into(),
            error: None,
        };
        log.log(&entry).unwrap();
    }
    c.bench_function("in_memory_audit_log_entries", |b| {
        b.iter(|| {
            let entries = log.entries();
            std::hint::black_box(entries);
        });
    });
}

criterion_group!(
    benches,
    bench_email_builder_basic,
    bench_email_builder_multiple_recipients,
    bench_email_builder_with_cc_and_bcc,
    bench_email_builder_with_attachment,
    bench_email_builder_with_bytes_attachment,
    bench_email_log_entry_creation,
    bench_in_memory_audit_log_append,
    bench_in_memory_audit_log_entries,
);
criterion_main!(benches);
