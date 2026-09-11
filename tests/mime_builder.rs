// Integration tests: unwrap is acceptable for test assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! MIME builder integration tests: parse output back and assert structure,
//! streaming attachment tests against real temp files, and golden fixtures.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use mailkit::message::EmailMessage;
use mailkit::mime::{MimeBuilder, MimeMessage};

// ---------------------------------------------------------------------------
// Deterministic builder + minimal MIME parser (test-side)
// ---------------------------------------------------------------------------

/// Pin date, message id, and boundaries for reproducible output.
fn deterministic(builder: MimeBuilder) -> MimeBuilder {
    builder
        .date(
            chrono::DateTime::parse_from_rfc2822("Fri, 11 Sep 2026 12:00:00 +0000")
                .unwrap()
                .into(),
        )
        .message_id("<golden@mailkit.local>")
        .boundary("=_mailkit_root")
        .fixed_boundaries()
}

#[derive(Debug)]
struct Part {
    headers: HashMap<String, String>,
    body: String,
    subparts: Vec<Part>,
}

impl Part {
    fn content_type(&self) -> &str {
        self.headers
            .get("Content-Type")
            .map(String::as_str)
            .unwrap_or("text/plain")
    }
    fn is_multipart(&self) -> bool {
        self.content_type().starts_with("multipart/")
    }
}

/// Unfold header lines, split headers from body, and split multiparts.
fn parse(input: &str) -> Part {
    let (head, body) = input.split_once("\r\n\r\n").expect("header/body separator");
    let mut headers = HashMap::new();
    let mut unfolded = String::new();
    for line in head.split("\r\n") {
        if line.starts_with(' ') || line.starts_with('\t') {
            unfolded.push(' ');
            unfolded.push_str(line.trim_start());
        } else {
            if !unfolded.is_empty() {
                push_header(&mut headers, &unfolded);
            }
            unfolded = line.to_owned();
        }
    }
    if !unfolded.is_empty() {
        push_header(&mut headers, &unfolded);
    }

    let mut subparts = Vec::new();
    if let Some(ct) = headers.get("Content-Type") {
        if let Some(boundary) = ct
            .split("boundary=\"")
            .nth(1)
            .and_then(|b| b.split('"').next())
        {
            let delimiter = format!("--{boundary}");
            for raw in split_multipart(body, &delimiter) {
                subparts.push(parse(raw));
            }
        }
    }
    Part {
        headers,
        body: body.to_owned(),
        subparts,
    }
}

fn push_header(headers: &mut HashMap<String, String>, line: &str) {
    if let Some((name, value)) = line.split_once(':') {
        headers.insert(name.trim().to_owned(), value.trim().to_owned());
    }
}

/// Split a multipart body on a delimiter, dropping the epilogue.
fn split_multipart<'a>(body: &'a str, delimiter: &str) -> Vec<&'a str> {
    let mut parts = Vec::new();
    let mut current: Option<&str> = None;
    for segment in body.split(delimiter) {
        // A segment starting with "--" after the delimiter is the closer.
        let segment = segment.strip_prefix('\r').unwrap_or(segment);
        let segment = segment.strip_prefix('\n').unwrap_or(segment);
        if segment.starts_with("--") {
            break;
        }
        if let Some(part) = current.take() {
            parts.push(part);
        }
        current = Some(segment);
    }
    if let Some(part) = current {
        parts.push(part);
    }
    parts
        .into_iter()
        .filter(|p| !p.is_empty())
        // The CRLF preceding a delimiter belongs to the delimiter
        // (RFC 2046): strip it so part bodies are exact.
        .map(|p| p.strip_suffix("\r\n").unwrap_or(p))
        .collect()
}

// ---------------------------------------------------------------------------
// Structure: parse back and assert
// ---------------------------------------------------------------------------

#[tokio::test]
async fn parses_text_html_alternative() {
    let msg = deterministic(
        MimeBuilder::new()
            .from("a@b.com")
            .to("c@d.com")
            .subject("Alt")
            .text_body("plain")
            .html_body("<b>html</b>"),
    )
    .build()
    .await
    .unwrap();

    let part = parse(msg.as_str());
    assert!(part.is_multipart());
    assert!(part.content_type().starts_with("multipart/alternative"));
    assert_eq!(part.subparts.len(), 2);
    assert!(part.subparts[0].content_type().starts_with("text/plain"));
    assert!(part.subparts[1].content_type().starts_with("text/html"));
    assert_eq!(part.subparts[0].body, "plain");
    assert_eq!(part.subparts[1].body, "<b>html</b>");
}

#[tokio::test]
async fn parses_mixed_with_alternative_and_attachments() {
    let msg = deterministic(
        MimeBuilder::new()
            .from("a@b.com")
            .to("c@d.com")
            .subject("Mixed")
            .text_body("plain")
            .html_body("<b>html</b>")
            .attach_bytes("one.txt", "text/plain", b"first")
            .attach_bytes("two.bin", "application/octet-stream", [1, 2, 3]),
    )
    .build()
    .await
    .unwrap();

    let root = parse(msg.as_str());
    assert!(root.content_type().starts_with("multipart/mixed"));
    assert_eq!(root.subparts.len(), 3);

    let alt = &root.subparts[0];
    assert!(alt.content_type().starts_with("multipart/alternative"));
    assert_eq!(alt.subparts.len(), 2);

    let att1 = &root.subparts[1];
    assert_eq!(
        att1.headers.get("Content-Disposition").map(String::as_str),
        Some("attachment; filename=\"one.txt\"")
    );
    assert_eq!(
        att1.headers
            .get("Content-Transfer-Encoding")
            .map(String::as_str),
        Some("base64")
    );
    // "first" base64 encoded.
    assert_eq!(att1.body, "Zmlyc3Q=");

    let att2 = &root.subparts[2];
    assert_eq!(att2.body, "AQID"); // [1,2,3]
}

#[tokio::test]
async fn parses_related_inline_images() {
    let png = [0x89u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    let msg = deterministic(
        MimeBuilder::new()
            .from("a@b.com")
            .to("c@d.com")
            .subject("Related")
            .text_body("see chart")
            .html_body("<img src=\"cid:chart\">")
            .attach_inline_bytes("chart.png", "image/png", png, "chart"),
    )
    .build()
    .await
    .unwrap();

    let root = parse(msg.as_str());
    assert!(root.content_type().starts_with("multipart/related"));

    let alt = &root.subparts[0];
    assert!(alt.content_type().starts_with("multipart/alternative"));

    let image = &root.subparts[1];
    assert_eq!(
        image.headers.get("Content-ID").map(String::as_str),
        Some("<chart>")
    );
    assert_eq!(
        image.headers.get("Content-Disposition").map(String::as_str),
        Some("inline; filename=\"chart.png\"")
    );
    assert_eq!(b64_decode(image.body.trim()), png);
}

fn b64_decode(input: &str) -> Vec<u8> {
    const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let cleaned: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '=')
        .collect();
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in cleaned.chars() {
        acc = (acc << 6) | ALPHABET.find(c).expect("base64 char") as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xFF) as u8);
        }
    }
    out
}

#[tokio::test]
async fn headers_are_correct() {
    let msg = deterministic(
        MimeBuilder::new()
            .from("Alice <a@b.com>")
            .to("c@d.com")
            .to("e@f.com")
            .cc("g@h.com")
            .subject("Hi")
            .header("Reply-To", "noreply@b.com")
            .text_body("x"),
    )
    .build()
    .await
    .unwrap();
    let s = msg.as_str();
    assert!(s.contains("From: Alice <a@b.com>\r\n"));
    assert!(s.contains("To: c@d.com, e@f.com\r\n"));
    assert!(s.contains("Cc: g@h.com\r\n"));
    assert!(s.contains("Subject: Hi\r\n"));
    assert!(s.contains("Date: Fri, 11 Sep 2026 12:00:00 +0000\r\n"));
    assert!(s.contains("Message-ID: <golden@mailkit.local>\r\n"));
    assert!(s.contains("MIME-Version: 1.0\r\n"));
    assert!(s.contains("Reply-To: noreply@b.com\r\n"));
}

#[tokio::test]
async fn non_ascii_text_body_is_base64() {
    let msg = deterministic(
        MimeBuilder::new()
            .text_body("Ünïcode body — with an em dash")
            .html_body("<p>Grüße</p>"),
    )
    .build()
    .await
    .unwrap();

    let root = parse(msg.as_str());
    let text = &root.subparts[0];
    assert_eq!(
        text.headers
            .get("Content-Transfer-Encoding")
            .map(String::as_str),
        Some("base64")
    );
    let decoded = String::from_utf8(b64_decode(&text.body)).unwrap();
    assert_eq!(decoded, "Ünïcode body — with an em dash");
}

// ---------------------------------------------------------------------------
// Streaming attachments (real files, chunked reads, size guard)
// ---------------------------------------------------------------------------

fn temp_file(contents: &[u8]) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let name = format!(
        "mailkit-mime-test-{}-{}.bin",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let path = std::env::temp_dir().join(name);
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(contents).unwrap();
    path
}

/// Pseudorandom-but-deterministic bytes covering chunk boundaries.
fn pseudo_bytes(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 31 % 251) as u8).collect()
}

#[tokio::test]
async fn streamed_file_matches_in_memory_encoding() {
    for len in [0usize, 1, 2, 3, 4, 5, 24_575, 24_576, 24_577, 100_003] {
        let data = pseudo_bytes(len);
        let path = temp_file(&data);

        let streamed = MimeBuilder::new()
            .text_body("x")
            .attach_file(&path)
            .await
            .unwrap()
            .build()
            .await
            .unwrap();

        // Parse the message and decode the attachment back.
        let root = parse(streamed.as_str());
        assert!(root.content_type().starts_with("multipart/mixed"));
        let att = &root.subparts[1];
        let decoded = b64_decode(&att.body);
        assert_eq!(decoded, data, "stream mismatch at len {len}");

        std::fs::remove_file(&path).unwrap();
    }
}

#[tokio::test]
async fn size_guard_rejects_large_files() {
    let data = vec![7u8; 4096];
    let path = temp_file(&data);

    let err = MimeBuilder::new()
        .text_body("x")
        .max_attachment_size(1024)
        .attach_file(&path)
        .await
        .unwrap()
        .build()
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("exceeding the 1024 byte limit"),
        "{err}"
    );

    std::fs::remove_file(&path).unwrap();
}

#[tokio::test]
async fn missing_file_is_io_error() {
    let result = MimeBuilder::new()
        .text_body("x")
        .attach_file_with(
            "/nonexistent/path/data.bin",
            "data.bin",
            "application/octet-stream",
        )
        .await
        .unwrap()
        .build()
        .await;
    assert!(matches!(result, Err(mailkit::EmailError::Io(_))));
}

#[tokio::test]
async fn from_message_streams_path_attachments() {
    let data = b"queued file content".to_vec();
    let path = temp_file(&data);

    let message = EmailMessage::builder()
        .from("a@b.com")
        .to("c@d.com")
        .subject("Queue holds path refs")
        .text_body("see attachment")
        .attachment(mailkit::message::Attachment {
            filename: "queued.txt".into(),
            content_type: "text/plain".into(),
            path: Some(path.clone()),
            bytes: None,
        })
        .build()
        .unwrap();

    // The queue can safely hold this message: bytes are read at send time.
    let provider: Box<dyn mailkit::provider::MailProvider> = Box::new(PathProvider);
    let client = mailkit::EmailClient::new(provider);
    client.queue().enqueue(message).await;
    client.queue().process(client.provider()).await.unwrap();
    assert!(client.queue().is_empty().await);
    // The file must still exist afterwards (streamed, not consumed).
    assert!(path.exists());
    std::fs::remove_file(&path).unwrap();
}

/// Provider that renders queued messages through the MIME builder.
struct PathProvider;

impl mailkit::provider::MailProvider for PathProvider {
    fn send<'a>(
        &'a self,
        message: &'a EmailMessage,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<mailkit::SendReceipt, mailkit::EmailError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            let mime = MimeBuilder::from_message(message).build().await?;
            assert!(mime.as_str().contains("cXVldWVkIGZpbGUgY29udGVudA==")); // "queued file content"
            Ok(mailkit::SendReceipt::new("path", None))
        })
    }

    fn name(&self) -> &str {
        "path"
    }
}

#[tokio::test]
async fn output_is_crlf_only() {
    let msg = deterministic(
        MimeBuilder::new()
            .text_body("a\nb\nc")
            .html_body("<p>x</p>")
            .attach_bytes("f.txt", "text/plain", b"y\nz"),
    )
    .build()
    .await
    .unwrap();
    // No bare LF: every newline in the output is CRLF.
    let bytes = msg.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' {
            assert!(i > 0 && bytes[i - 1] == b'\r', "bare LF at offset {i}");
        }
    }
}

// ---------------------------------------------------------------------------
// Golden fixtures
// ---------------------------------------------------------------------------

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

#[tokio::test]
async fn golden_text_html_attachments() {
    let msg = deterministic(
        MimeBuilder::new()
            .from("Alice <alice@example.com>")
            .to("bob@example.com")
            .subject("Quarterly report")
            .text_body("Please see the attached report.")
            .html_body("<p>Please see the <b>attached</b> report.</p>")
            .attach_bytes("report.txt", "text/plain", b"report contents")
            .attach_bytes("data.bin", "application/octet-stream", [0x00, 0x01, 0x02]),
    )
    .build()
    .await
    .unwrap();
    assert_eq!(msg.as_str(), fixture("text_html_attachments.eml"));
}

#[tokio::test]
async fn golden_inline_image() {
    let png = [0x89u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    let msg = deterministic(
        MimeBuilder::new()
            .from("alice@example.com")
            .to("bob@example.com")
            .subject("Chart inside")
            .html_body("<p>Inline: <img src=\"cid:chart\"></p>")
            .text_body("Inline chart attached.")
            .attach_inline_bytes("chart.png", "image/png", png, "chart"),
    )
    .build()
    .await
    .unwrap();
    assert_eq!(msg.as_str(), fixture("inline_image.eml"));
}

#[tokio::test]
async fn golden_unicode_subject() {
    let msg = deterministic(
        MimeBuilder::new()
            .from("alice@example.com")
            .to("bob@example.com")
            .subject("Grüße aus München")
            .text_body("Servus!")
            .attach_bytes("note.txt", "text/plain", b"hello"),
    )
    .build()
    .await
    .unwrap();
    assert_eq!(msg.as_str(), fixture("unicode_subject.eml"));
}

/// Keep `MimeMessage` helpers exercised.
#[tokio::test]
async fn mime_message_accessors() {
    let msg: MimeMessage = deterministic(MimeBuilder::new().text_body("x"))
        .build()
        .await
        .unwrap();
    assert_eq!(msg.as_bytes(), msg.as_str().as_bytes());
    assert_eq!(msg.clone().into_string(), msg.as_str());
}
