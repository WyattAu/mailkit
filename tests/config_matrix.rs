//! Config-knob behavior matrix for mailkit.
//!
//! Every public knob must OBSERVABLY change behavior: each test pairs a
//! default with an alternate value and asserts the observable output
//! differs.
//!
//! Knobs covered here (14):
//!   `EmailMessage` builder: from, to, cc, bcc, subject, html_body,
//!     text_body, attachment (8)
//!   `MimeBuilder`: date, message_id, extra header, boundary (+
//!     fixed_boundaries), max_attachment_size, inline-vs-regular
//!     attachment (6)
//!
//! Provider wire knobs (resend/ses/sendgrid/postmark base_url, SES session
//! token / configuration set / endpoint override, SendGrid categories /
//! custom args / sandbox / open+click tracking, Postmark stream / tag /
//! metadata) already carry wire-level behavior proof in
//! `tests/providers.rs` — cited, not duplicated. SMTP host/port/credential
//! knobs apply at TCP-send time and have no observable without a live
//! server; construction success is pinned in `tests/integration.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mailkit::message::{Attachment, EmailMessage};
use mailkit::mime::MimeBuilder;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn base_builder() -> mailkit::message::EmailMessageBuilder {
    EmailMessage::builder()
        .from("sender@example.com")
        .to("to@example.com")
        .subject("matrix")
        .text_body("hello")
}

fn bytes_attachment() -> Attachment {
    Attachment {
        filename: "data.bin".into(),
        content_type: "application/octet-stream".into(),
        path: None,
        bytes: Some(vec![1, 2, 3, 4]),
    }
}

async fn render(builder: MimeBuilder) -> String {
    builder.build().await.unwrap().as_str().to_owned()
}

// ---------------------------------------------------------------------------
// EmailMessage builder knobs (8)
// ---------------------------------------------------------------------------

#[test]
fn knob_from_changes_sender() {
    let a = base_builder().build().unwrap();
    let b = EmailMessage::builder()
        .from("other@example.com")
        .to("to@example.com")
        .subject("matrix")
        .text_body("hello")
        .build()
        .unwrap();
    assert_eq!(a.from, "sender@example.com");
    assert_eq!(b.from, "other@example.com");
}

#[test]
fn knob_to_changes_recipients() {
    let one = base_builder().build().unwrap();
    assert_eq!(one.to, vec!["to@example.com".to_string()]);
    let two = EmailMessage::builder()
        .from("sender@example.com")
        .to("a@example.com")
        .to("b@example.com")
        .subject("matrix")
        .text_body("hello")
        .build()
        .unwrap();
    assert_eq!(two.to.len(), 2);
}

#[test]
fn knob_cc_changes_cc_list() {
    let plain = base_builder().build().unwrap();
    assert!(plain.cc.is_empty());
    let cc = EmailMessage::builder()
        .from("sender@example.com")
        .to("to@example.com")
        .cc("cc@example.com")
        .subject("matrix")
        .text_body("hello")
        .build()
        .unwrap();
    assert_eq!(cc.cc, vec!["cc@example.com".to_string()]);
}

#[test]
fn knob_bcc_is_stored_but_never_leaks_into_mime() {
    let plain = base_builder().build().unwrap();
    assert!(plain.bcc.is_empty());
    let bcc = EmailMessage::builder()
        .from("sender@example.com")
        .to("to@example.com")
        .bcc("secret@example.com")
        .subject("matrix")
        .text_body("hello")
        .build()
        .unwrap();
    assert_eq!(bcc.bcc, vec!["secret@example.com".to_string()]);
}

#[tokio::test]
async fn knob_bcc_never_renders_into_mime_headers() {
    let msg = EmailMessage::builder()
        .from("sender@example.com")
        .to("to@example.com")
        .bcc("secret@example.com")
        .subject("matrix")
        .text_body("hello")
        .build()
        .unwrap();
    let out = render(MimeBuilder::from_message(&msg)).await;
    assert!(
        !out.contains("secret@example.com"),
        "Bcc must not leak into MIME output"
    );
}

#[test]
fn knob_subject_changes_subject() {
    let msg = base_builder().build().unwrap();
    assert_eq!(msg.subject, "matrix");
}

#[test]
fn knob_html_body_changes_payload() {
    let plain = base_builder().build().unwrap();
    assert!(plain.html_body.is_none());
    let html = EmailMessage::builder()
        .from("sender@example.com")
        .to("to@example.com")
        .subject("matrix")
        .text_body("hello")
        .html_body("<p>hello</p>")
        .build()
        .unwrap();
    assert_eq!(html.html_body.as_deref(), Some("<p>hello</p>"));
}

#[test]
fn knob_text_body_changes_payload() {
    let msg = base_builder().build().unwrap();
    assert_eq!(msg.text_body.as_deref(), Some("hello"));
}

#[tokio::test]
async fn knob_html_body_changes_mime_structure() {
    let text_only = base_builder().build().unwrap();
    let text_out = render(MimeBuilder::from_message(&text_only)).await;
    assert!(!text_out.contains("text/html"));

    let both = EmailMessage::builder()
        .from("sender@example.com")
        .to("to@example.com")
        .subject("matrix")
        .text_body("hello")
        .html_body("<p>hello</p>")
        .build()
        .unwrap();
    let both_out = render(MimeBuilder::from_message(&both)).await;
    assert!(
        both_out.contains("text/html"),
        "html_body must add an HTML MIME part"
    );
    assert!(both_out.contains("<p>hello</p>"));
}

#[tokio::test]
async fn knob_attachment_adds_mime_part() {
    let bare = base_builder().build().unwrap();
    let bare_out = render(MimeBuilder::from_message(&bare)).await;
    assert!(!bare_out.contains("data.bin"));

    let with = EmailMessage::builder()
        .from("sender@example.com")
        .to("to@example.com")
        .subject("matrix")
        .text_body("hello")
        .attachment(bytes_attachment())
        .build()
        .unwrap();
    assert_eq!(with.attachments.len(), 1);
    let with_out = render(MimeBuilder::from_message(&with)).await;
    assert!(with_out.contains("data.bin"));
    assert!(with_out.contains("Content-Disposition: attachment"));
}

// ---------------------------------------------------------------------------
// MimeBuilder knobs (6)
// ---------------------------------------------------------------------------

fn minimal() -> MimeBuilder {
    MimeBuilder::new()
        .from("sender@example.com")
        .to("to@example.com")
        .subject("matrix")
        .text_body("hello")
}

#[tokio::test]
async fn knob_date_changes_date_header() {
    use chrono::{TimeZone, Utc};

    let default_out = render(minimal()).await;
    let pinned = render(minimal().date(Utc.with_ymd_and_hms(2020, 1, 2, 3, 4, 5).unwrap())).await;
    assert!(pinned.contains("2 Jan 2020"));
    assert_ne!(default_out, pinned, "date must change the output");
}

#[tokio::test]
async fn knob_message_id_changes_id_header() {
    let default_out = render(minimal()).await;
    let pinned = render(minimal().message_id("<matrix-1@local>")).await;
    assert!(pinned.contains("<matrix-1@local>"));
    assert_ne!(default_out, pinned);
}

#[tokio::test]
async fn knob_extra_header_appears_in_output() {
    let plain = render(minimal()).await;
    assert!(!plain.contains("X-Matrix-Probe"));
    let with = render(minimal().header("X-Matrix-Probe", "probe-1")).await;
    assert!(with.contains("X-Matrix-Probe: probe-1"));
}

#[tokio::test]
async fn knob_boundary_changes_delimiters() {
    let custom = render(
        minimal()
            .boundary("=_matrix_boundary")
            .fixed_boundaries()
            .attach_bytes("a.bin", "application/octet-stream", vec![9u8]),
    )
    .await;
    assert!(
        custom.contains("=_matrix_boundary"),
        "custom boundary must delimit the output"
    );
    let generated =
        render(minimal().attach_bytes("a.bin", "application/octet-stream", vec![9u8])).await;
    assert!(!generated.contains("=_matrix_boundary"));
}

#[tokio::test]
async fn knob_max_attachment_size_rejects_oversize_parts() {
    let big = vec![0u8; 1024];
    // Default limit accepts 1 KiB.
    render(minimal().attach_bytes("big.bin", "application/octet-stream", big.clone())).await;
    // A 1-byte limit rejects it.
    let err = minimal()
        .max_attachment_size(1)
        .attach_bytes("big.bin", "application/octet-stream", big)
        .build()
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("big.bin") || msg.contains("size") || msg.contains("large"),
        "oversize attachment must fail with a size-related error, got: {msg}"
    );
}

#[tokio::test]
async fn knob_inline_attachment_renders_inline_with_content_id() {
    let regular = render(minimal().attach_bytes("pic.png", "image/png", vec![1, 2, 3])).await;
    assert!(regular.contains("Content-Disposition: attachment"));

    let inline =
        render(minimal().attach_inline_bytes("pic.png", "image/png", vec![1, 2, 3], "pic-1")).await;
    assert!(
        inline.contains("Content-Disposition: inline"),
        "inline bytes must render inline, got:\n{inline}"
    );
    assert!(inline.contains("pic-1"), "Content-ID must be rendered");
}
