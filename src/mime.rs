//! Self-contained MIME multipart message builder.
//!
//! Builds RFC 2045–2047 conformant messages — `multipart/mixed` with
//! `multipart/alternative` (text + HTML) bodies, inline images with
//! `Content-ID`s, and base64 attachments — **without** depending on lettre's
//! builder. Output uses CRLF line endings, 76-column-folded base64 transfer
//! encoding, and RFC 2047 encoded-words for non-ASCII headers.
//!
//! The resulting [`MimeMessage`] can be sent raw by providers that accept
//! full MIME (AWS SES `Raw`, Postmark, SMTP) and gives the SMTP path a
//! consistent builder.
//!
//! Structure:
//!
//! ```text
//! no bodies, no attachments   → empty text/plain part
//! text and/or html only       → single part, or multipart/alternative
//! + inline parts (Content-ID) → wrapped in multipart/related
//! + attachments               → wrapped in multipart/mixed
//! ```
//!
//! Attachments provided as file paths are streamed in chunks (never fully
//! buffered as raw bytes), base64-encoded incrementally, and guarded by a
//! configurable per-attachment size limit
//! ([`crate::mime::DEFAULT_MAX_ATTACHMENT_SIZE`]).
//! This makes path-based [`crate::message::Attachment`]s safe to keep in the
//! [`crate::EmailQueue`]: raw bytes are only read at send time.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::base64::{self, FoldingEncoder};
use crate::error::EmailError;
#[cfg(any(feature = "resend", feature = "sendgrid", feature = "postmark"))]
use crate::message::Attachment;
use crate::message::EmailMessage;

/// Default per-attachment size cap: 25 MiB.
pub const DEFAULT_MAX_ATTACHMENT_SIZE: u64 = 25 * 1024 * 1024;

/// Streaming read chunk size; a multiple of 3 so base64 triples align.
const STREAM_CHUNK: usize = 3 * 8192;

/// A fully built MIME message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MimeMessage {
    raw: String,
}

impl MimeMessage {
    /// The full RFC 5322/MIME message as a string (CRLF line endings).
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// The full message as bytes, ready for a raw-MIME transport.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.raw.as_bytes()
    }

    /// Consume into the raw message string.
    #[must_use]
    pub fn into_string(self) -> String {
        self.raw
    }
}

/// Builder for MIME multipart messages.
///
/// ```
/// use mailkit::mime::MimeBuilder;
///
/// # async fn demo() -> Result<(), mailkit::EmailError> {
/// let message = MimeBuilder::new()
///     .from("Alice <alice@example.com>")
///     .to("bob@example.com")
///     .subject("Report")
///     .text_body("See attached.")
///     .html_body("<p>See <b>attached</b>.</p>")
///     .attach_bytes("report.pdf", "application/pdf", b"%PDF-1.4 fake")
///     .build()
///     .await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct MimeBuilder {
    from: Option<String>,
    to: Vec<String>,
    cc: Vec<String>,
    subject: Option<String>,
    text: Option<String>,
    html: Option<String>,
    date: Option<DateTime<Utc>>,
    message_id: Option<String>,
    extra_headers: Vec<(String, String)>,
    root_boundary: Option<String>,
    fixed_boundaries: bool,
    max_attachment_size: u64,
    attachments: Vec<PlannedPart>,
    inline: Vec<PlannedPart>,
}

#[derive(Debug, Clone)]
struct PlannedPart {
    filename: String,
    content_type: String,
    content_id: Option<String>,
    source: PartSource,
}

#[derive(Debug, Clone)]
enum PartSource {
    Bytes(Vec<u8>),
    Path(PathBuf),
}

impl Default for MimeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl MimeBuilder {
    /// Create a new empty MIME builder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            from: None,
            to: Vec::new(),
            cc: Vec::new(),
            subject: None,
            text: None,
            html: None,
            date: None,
            message_id: None,
            extra_headers: Vec::new(),
            root_boundary: None,
            fixed_boundaries: false,
            max_attachment_size: DEFAULT_MAX_ATTACHMENT_SIZE,
            attachments: Vec::new(),
            inline: Vec::new(),
        }
    }

    /// Seed the builder from a provider-neutral [`EmailMessage`].
    ///
    /// Body fields map directly; attachments referencing raw bytes are used
    /// as-is and path attachments are streamed (with the size guard) when
    /// [`MimeBuilder::build`] is awaited.
    #[must_use]
    pub fn from_message(message: &EmailMessage) -> Self {
        let mut builder = Self::new()
            .from(message.from.clone())
            .subject(message.subject.clone())
            .text_body_opt(message.text_body.clone())
            .html_body_opt(message.html_body.clone());
        for addr in &message.to {
            builder = builder.to(addr.clone());
        }
        for addr in &message.cc {
            builder = builder.cc(addr.clone());
        }
        for att in &message.attachments {
            let source = match &att.bytes {
                Some(bytes) => PartSource::Bytes(bytes.clone()),
                None => PartSource::Path(att.path.clone().unwrap_or_default()),
            };
            builder.attachments.push(PlannedPart {
                filename: att.filename.clone(),
                content_type: att.content_type.clone(),
                content_id: None,
                source,
            });
        }
        builder
    }

    /// Set the sender (an RFC 5322 address, optionally with display name).
    #[must_use]
    pub fn from(mut self, from: impl Into<String>) -> Self {
        self.from = Some(from.into());
        self
    }

    /// Add a recipient.
    #[must_use]
    pub fn to(mut self, to: impl Into<String>) -> Self {
        self.to.push(to.into());
        self
    }

    /// Add a carbon-copy recipient.
    #[must_use]
    pub fn cc(mut self, cc: impl Into<String>) -> Self {
        self.cc.push(cc.into());
        self
    }

    /// Set the subject. Non-ASCII subjects are RFC 2047 encoded.
    #[must_use]
    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    /// Set the plain-text body.
    #[must_use]
    pub fn text_body(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Set the HTML body.
    #[must_use]
    pub fn html_body(mut self, html: impl Into<String>) -> Self {
        self.html = Some(html.into());
        self
    }

    fn text_body_opt(mut self, text: Option<String>) -> Self {
        self.text = text;
        self
    }

    fn html_body_opt(mut self, html: Option<String>) -> Self {
        self.html = html;
        self
    }

    /// Set the `Date` header explicitly. Defaults to now.
    #[must_use]
    pub fn date(mut self, date: DateTime<Utc>) -> Self {
        self.date = Some(date);
        self
    }

    /// Set the `Message-ID` header explicitly (full value, including angle
    /// brackets if desired). Defaults to a generated id.
    #[must_use]
    pub fn message_id(mut self, message_id: impl Into<String>) -> Self {
        self.message_id = Some(message_id.into());
        self
    }

    /// Add an arbitrary extra top-level header (e.g. `Reply-To`, `X-Priority`).
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra_headers.push((name.into(), value.into()));
        self
    }

    /// Override the root `multipart/mixed` boundary instead of generating one.
    #[must_use]
    pub fn boundary(mut self, boundary: impl Into<String>) -> Self {
        self.root_boundary = Some(boundary.into());
        self
    }

    /// Test hook: use fixed per-subtype nested boundaries instead of random
    /// ones so output is fully reproducible for golden-file tests.
    #[doc(hidden)]
    #[must_use]
    pub fn fixed_boundaries(mut self) -> Self {
        self.fixed_boundaries = true;
        self
    }

    /// Set the per-attachment size guard. Default: [`DEFAULT_MAX_ATTACHMENT_SIZE`].
    #[must_use]
    pub fn max_attachment_size(mut self, max_bytes: u64) -> Self {
        self.max_attachment_size = max_bytes;
        self
    }

    /// Attach bytes as `Content-Disposition: attachment`.
    #[must_use]
    pub fn attach_bytes(
        mut self,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        bytes: impl Into<Vec<u8>>,
    ) -> Self {
        self.attachments.push(PlannedPart {
            filename: filename.into(),
            content_type: content_type.into(),
            content_id: None,
            source: PartSource::Bytes(bytes.into()),
        });
        self
    }

    /// Attach inline bytes with a `Content-ID` (referenced from HTML as
    /// `<img src="cid:...">`); rendered as `Content-Disposition: inline`.
    #[must_use]
    pub fn attach_inline_bytes(
        mut self,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        bytes: impl Into<Vec<u8>>,
        content_id: impl Into<String>,
    ) -> Self {
        self.inline.push(PlannedPart {
            filename: filename.into(),
            content_type: content_type.into(),
            content_id: Some(content_id.into()),
            source: PartSource::Bytes(bytes.into()),
        });
        self
    }

    /// Stream-attach a file as `Content-Disposition: attachment`. The file
    /// name is derived from the path.
    ///
    /// # Errors
    /// Returns [`EmailError::Io`] if the file cannot be opened, and a
    /// provider error if the file exceeds [`MimeBuilder::max_attachment_size`].
    pub async fn attach_file(self, path: impl AsRef<Path>) -> Result<Self, EmailError> {
        let path = path.as_ref();
        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "attachment.bin".to_owned());
        self.attach_file_with(path, filename, "application/octet-stream")
            .await
    }

    /// Stream-attach a file with an explicit file name and content type.
    ///
    /// # Errors
    /// See [`MimeBuilder::attach_file`].
    pub async fn attach_file_with(
        mut self,
        path: impl AsRef<Path>,
        filename: impl Into<String>,
        content_type: impl Into<String>,
    ) -> Result<Self, EmailError> {
        self.attachments.push(PlannedPart {
            filename: filename.into(),
            content_type: content_type.into(),
            content_id: None,
            source: PartSource::Path(path.as_ref().to_path_buf()),
        });
        Ok(self)
    }

    /// Attach an inline (streamed) file with a `Content-ID`.
    ///
    /// # Errors
    /// See [`MimeBuilder::attach_file`].
    pub async fn attach_inline_file_with(
        mut self,
        path: impl AsRef<Path>,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        content_id: impl Into<String>,
    ) -> Result<Self, EmailError> {
        self.inline.push(PlannedPart {
            filename: filename.into(),
            content_type: content_type.into(),
            content_id: Some(content_id.into()),
            source: PartSource::Path(path.as_ref().to_path_buf()),
        });
        Ok(self)
    }

    /// Build the MIME message.
    ///
    /// # Errors
    /// Returns [`EmailError::Io`] for unreadable attachment files and a
    /// provider error when an attachment exceeds the size guard.
    pub async fn build(self) -> Result<MimeMessage, EmailError> {
        let mut attachments = Vec::with_capacity(self.attachments.len());
        for part in &self.attachments {
            let encoded = self.encode_part(part).await?;
            attachments.push(LoadedPart {
                part: part.clone(),
                encoded,
            });
        }
        let mut inline = Vec::with_capacity(self.inline.len());
        for part in &self.inline {
            let encoded = self.encode_part(part).await?;
            inline.push(LoadedPart {
                part: part.clone(),
                encoded,
            });
        }

        // Core body: text, html, or multipart/alternative of both (text
        // first, HTML last — the most-representative part goes last per
        // RFC 2046). An empty text part keeps body-less messages conformant.
        let text = self.text.clone().unwrap_or_default();
        let core: MimeNode = match (&self.text, &self.html) {
            (Some(_), Some(html)) => MimeNode::Multi {
                subtype: "alternative",
                boundary: self.boundary_for("alternative", &[&text, html]),
                parts: vec![text_leaf(&text), html_leaf(html)],
            },
            (Some(_), None) => text_leaf(&text),
            (None, Some(html)) => html_leaf(html),
            (None, None) => text_leaf(""),
        };

        // Wrap inline parts in multipart/related around the core body.
        let with_inline = if inline.is_empty() {
            core
        } else {
            MimeNode::Multi {
                subtype: "related",
                boundary: self.boundary_for("related", &[]),
                parts: std::iter::once(core)
                    .chain(
                        inline
                            .iter()
                            .map(|p| MimeNode::Leaf(render_binary(p, true))),
                    )
                    .collect(),
            }
        };

        // Wrap attachments in multipart/mixed.
        let mut root = if attachments.is_empty() {
            with_inline
        } else {
            MimeNode::Multi {
                subtype: "mixed",
                boundary: self.boundary_for("mixed", &[]),
                parts: std::iter::once(with_inline)
                    .chain(
                        attachments
                            .iter()
                            .map(|p| MimeNode::Leaf(render_binary(p, false))),
                    )
                    .collect(),
            }
        };

        // An explicit root boundary override applies to whichever multipart
        // ends up at the root (mixed with attachments, else alternative).
        if let (Some(boundary), MimeNode::Multi { boundary: b, .. }) =
            (&self.root_boundary, &mut root)
        {
            *b = boundary.clone();
        }

        // The root node's entity headers (Content-Type etc.) join the
        // top-level header block; its body follows the blank line, per
        // RFC 5322/MIME structure.
        let (node_headers, node_body) = render_node(&root);
        let mut raw = String::with_capacity(4096);
        raw.push_str(&self.render_headers());
        raw.push_str(&node_headers);
        raw.push_str("\r\n");
        raw.push_str(&node_body);
        Ok(MimeMessage { raw })
    }

    async fn encode_part(&self, part: &PlannedPart) -> Result<String, EmailError> {
        match &part.source {
            PartSource::Bytes(bytes) => {
                if bytes.len() as u64 > self.max_attachment_size {
                    return Err(too_large(
                        &part.filename,
                        bytes.len() as u64,
                        self.max_attachment_size,
                    ));
                }
                let mut encoder = FoldingEncoder::new();
                for chunk in bytes.chunks(STREAM_CHUNK) {
                    encoder.push(chunk);
                }
                Ok(encoder.finish())
            }
            PartSource::Path(path) => {
                stream_base64_folded(path, self.max_attachment_size, &part.filename).await
            }
        }
    }

    fn boundary_for(&self, subtype: &'static str, contents: &[&str]) -> String {
        for _ in 0..8 {
            let candidate = if self.fixed_boundaries {
                format!("=_mailkit_fixed_{subtype}")
            } else {
                format!("=_mailkit_{}", base64::random_hex(24))
            };
            if !contents.iter().any(|c| c.contains(&candidate)) {
                return candidate;
            }
        }
        format!("=_mailkit_{subtype}_fallback")
    }

    fn render_headers(&self) -> String {
        let mut out = String::new();
        if let Some(from) = &self.from {
            out.push_str(&format!("From: {}\r\n", sanitize_header(from)));
        }
        if !self.to.is_empty() {
            let to: Vec<String> = self.to.iter().map(|a| sanitize_header(a)).collect();
            out.push_str(&format!("To: {}\r\n", to.join(", ")));
        }
        if !self.cc.is_empty() {
            let cc: Vec<String> = self.cc.iter().map(|a| sanitize_header(a)).collect();
            out.push_str(&format!("Cc: {}\r\n", cc.join(", ")));
        }
        if let Some(subject) = &self.subject {
            out.push_str(&format!("Subject: {}\r\n", encode_header_value(subject)));
        }
        let date = self.date.unwrap_or_else(Utc::now);
        out.push_str(&format!("Date: {}\r\n", date.to_rfc2822()));
        match &self.message_id {
            Some(id) => out.push_str(&format!("Message-ID: {}\r\n", sanitize_header(id))),
            None => out.push_str(&format!(
                "Message-ID: <{}@mailkit.local>\r\n",
                base64::random_hex(24)
            )),
        }
        out.push_str("MIME-Version: 1.0\r\n");
        for (name, value) in &self.extra_headers {
            out.push_str(&format!(
                "{}: {}\r\n",
                sanitize_header(name),
                encode_header_value(value)
            ));
        }
        out
    }
}

struct LoadedPart {
    part: PlannedPart,
    encoded: String,
}

enum MimeNode {
    Leaf(String),
    Multi {
        subtype: &'static str,
        boundary: String,
        parts: Vec<MimeNode>,
    },
}

/// Render a node into entity headers (no trailing blank line) and body.
fn render_node(node: &MimeNode) -> (String, String) {
    match node {
        MimeNode::Leaf(rendered) => {
            let (headers, body) = rendered
                .split_once("\r\n\r\n")
                .unwrap_or((rendered.as_str(), ""));
            (format!("{headers}\r\n"), body.to_owned())
        }
        MimeNode::Multi {
            subtype,
            boundary,
            parts,
        } => {
            let headers = format!("Content-Type: multipart/{subtype}; boundary=\"{boundary}\"\r\n");
            let mut body = String::new();
            for part in parts {
                let (part_headers, part_body) = render_node(part);
                body.push_str(&format!("--{boundary}\r\n"));
                body.push_str(&part_headers);
                body.push_str("\r\n");
                body.push_str(&part_body);
                body.push_str("\r\n");
            }
            body.push_str(&format!("--{boundary}--\r\n"));
            (headers, body)
        }
    }
}

fn text_leaf(text: &str) -> MimeNode {
    if text_transfer_encoding(text) == "base64" {
        MimeNode::Leaf(format!(
            "Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n{}",
            fold_base64(text.as_bytes())
        ))
    } else {
        MimeNode::Leaf(format!(
            "Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 7bit\r\n\r\n{}",
            normalize_crlf(text)
        ))
    }
}

fn html_leaf(html: &str) -> MimeNode {
    if text_transfer_encoding(html) == "base64" {
        MimeNode::Leaf(format!(
            "Content-Type: text/html; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n{}",
            fold_base64(html.as_bytes())
        ))
    } else {
        MimeNode::Leaf(format!(
            "Content-Type: text/html; charset=utf-8\r\nContent-Transfer-Encoding: 7bit\r\n\r\n{}",
            normalize_crlf(html)
        ))
    }
}

/// Base64-encode with 76-column CRLF folding for transfer encoding.
fn fold_base64(bytes: &[u8]) -> String {
    let mut encoder = FoldingEncoder::new();
    for chunk in bytes.chunks(STREAM_CHUNK) {
        encoder.push(chunk);
    }
    encoder.finish()
}

/// Convert bare LF and lone CR to CRLF (RFC 5322 requires CRLF on the wire).
fn normalize_crlf(text: &str) -> String {
    if text.contains('\n') || text.contains('\r') {
        text.replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\n', "\r\n")
    } else {
        text.to_owned()
    }
}

/// ASCII text with no dangerously long lines is sent as `7bit`; anything
/// else is base64-folded.
fn text_transfer_encoding(text: &str) -> &'static str {
    if text.is_ascii() && !text.lines().any(|line| line.len() > 900) {
        "7bit"
    } else {
        "base64"
    }
}

fn render_binary(part: &LoadedPart, inline: bool) -> String {
    let disposition = if inline { "inline" } else { "attachment" };
    let mut out = format!(
        "Content-Type: {}; name=\"{}\"\r\nContent-Transfer-Encoding: base64\r\nContent-Disposition: {disposition}; filename=\"{}\"\r\n",
        part.part.content_type,
        sanitize_param(&part.part.filename),
        sanitize_param(&part.part.filename),
    );
    if let Some(cid) = &part.part.content_id {
        out.push_str(&format!("Content-ID: <{}>\r\n", sanitize_param(cid)));
    }
    out.push_str("\r\n");
    out.push_str(&part.encoded);
    out
}

/// Stream a file in chunks, base64-encoding incrementally with a size guard.
/// Async so large attachment reads never block the runtime.
async fn stream_base64_folded(
    path: &Path,
    max_size: u64,
    filename: &str,
) -> Result<String, EmailError> {
    use tokio::io::AsyncReadExt;

    let mut file = tokio::fs::File::open(path).await?;
    let mut encoder = FoldingEncoder::new();
    let mut buf = vec![0u8; STREAM_CHUNK];
    let mut total: u64 = 0;
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > max_size {
            return Err(too_large(filename, total, max_size));
        }
        encoder.push(&buf[..n]);
    }
    Ok(encoder.finish())
}

/// Load an [`Attachment`] as plain (unfolded) base64 for JSON APIs such as
/// SendGrid and Postmark. Bytes are encoded directly; file paths are
/// streamed with the given size guard.
#[cfg(any(feature = "resend", feature = "sendgrid", feature = "postmark"))]
pub(crate) async fn attachment_base64(
    attachment: &Attachment,
    max_size: u64,
) -> Result<String, EmailError> {
    if let Some(bytes) = &attachment.bytes {
        if bytes.len() as u64 > max_size {
            return Err(too_large(
                &attachment.filename,
                bytes.len() as u64,
                max_size,
            ));
        }
        return Ok(base64::encode(bytes));
    }
    let path = attachment.path.as_deref().ok_or_else(|| {
        EmailError::provider(format!(
            "attachment {:?} has neither bytes nor a path",
            attachment.filename
        ))
    })?;
    let folded = stream_base64_folded(path, max_size, &attachment.filename).await?;
    Ok(folded.replace("\r\n", ""))
}

fn too_large(filename: &str, size: u64, max: u64) -> EmailError {
    EmailError::provider(format!(
        "attachment {filename:?} is {size} bytes, exceeding the {max} byte limit"
    ))
}

/// Neutralize CR/LF in values destined for header lines (header injection guard).
fn sanitize_header(value: &str) -> String {
    value
        .chars()
        .map(|c| if c == '\r' || c == '\n' { ' ' } else { c })
        .collect()
}

/// Strip characters that would break a quoted RFC 2045 parameter.
fn sanitize_param(value: &str) -> String {
    value
        .chars()
        .filter(|c| *c != '"' && *c != '\r' && *c != '\n' && *c != '\\')
        .collect()
}

/// Encode a header value: passed through when ASCII, otherwise split into
/// RFC 2047 B-encoded words (`=?utf-8?B?...?=`) folded with CRLF + space.
fn encode_header_value(value: &str) -> String {
    let value = sanitize_header(value);
    if value.is_ascii() {
        return value;
    }
    // Each encoded word: "=?utf-8?B?" (10) + base64 + "?=" (2) ≤ 75 chars →
    // chunks of at most 45 source bytes (60 base64 chars).
    const MAX_CHUNK_BYTES: usize = 45;
    let mut words: Vec<String> = Vec::new();
    let mut chunk = String::new();
    let mut chunk_bytes = 0usize;
    for ch in value.chars() {
        if chunk_bytes + ch.len_utf8() > MAX_CHUNK_BYTES {
            words.push(b_encode(&chunk));
            chunk.clear();
            chunk_bytes = 0;
        }
        chunk.push(ch);
        chunk_bytes += ch.len_utf8();
    }
    if !chunk.is_empty() {
        words.push(b_encode(&chunk));
    }
    words.join("\r\n ")
}

fn b_encode(text: &str) -> String {
    format!("=?utf-8?B?{}?=", base64::encode(text.as_bytes()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// Pin date, message id, and root boundary so output is deterministic.
    fn deterministic(builder: MimeBuilder) -> MimeBuilder {
        builder
            .date(
                DateTime::parse_from_rfc2822("Fri, 11 Sep 2026 12:00:00 +0000")
                    .unwrap()
                    .with_timezone(&Utc),
            )
            .message_id("<test@mailkit.local>")
            .boundary("=_mailkit_root")
    }

    #[tokio::test]
    async fn plain_text_only_is_single_part() {
        let msg = deterministic(
            MimeBuilder::new()
                .from("a@b.com")
                .to("c@d.com")
                .subject("Hello")
                .text_body("Plain body"),
        )
        .build()
        .await
        .unwrap();
        let s = msg.as_str();
        assert!(s.starts_with("From: a@b.com\r\nTo: c@d.com\r\nSubject: Hello\r\n"));
        assert!(s.contains("MIME-Version: 1.0\r\n"));
        assert!(s.contains("Content-Type: text/plain; charset=utf-8\r\n"));
        assert!(s.contains("Content-Transfer-Encoding: 7bit\r\n\r\nPlain body"));
        assert!(!s.contains("boundary"));
        assert!(s.ends_with("\r\n\r\nPlain body"));
    }

    #[tokio::test]
    async fn html_only_single_part() {
        let msg = deterministic(MimeBuilder::new().html_body("<p>hi</p>"))
            .build()
            .await
            .unwrap();
        assert!(
            msg.as_str()
                .contains("Content-Type: text/html; charset=utf-8")
        );
        assert!(msg.as_str().ends_with("\r\n\r\n<p>hi</p>"));
    }

    #[tokio::test]
    async fn text_plus_html_is_alternative() {
        let msg = deterministic(
            MimeBuilder::new()
                .text_body("text")
                .html_body("<b>html</b>"),
        )
        .build()
        .await
        .unwrap();
        let s = msg.as_str();
        assert!(s.contains("Content-Type: multipart/alternative; boundary=\"=_mailkit_root\""));
        assert!(s.contains("--=_mailkit_root\r\nContent-Type: text/plain"));
        // Text part precedes the HTML part (most-representative last).
        assert!(s.find("text/plain").unwrap() < s.find("text/html").unwrap());
        assert!(s.contains("\r\n--=_mailkit_root--\r\n"));
    }

    #[tokio::test]
    async fn attachment_produces_mixed() {
        let msg = deterministic(
            MimeBuilder::new()
                .text_body("body")
                .html_body("<b>body</b>")
                .attach_bytes("notes.txt", "text/plain", b"attached content"),
        )
        .build()
        .await
        .unwrap();
        let s = msg.as_str();
        assert!(s.contains("Content-Type: multipart/mixed; boundary=\"=_mailkit_root\""));
        assert!(s.contains("Content-Type: text/plain; name=\"notes.txt\""));
        assert!(s.contains("Content-Disposition: attachment; filename=\"notes.txt\""));
        assert!(s.contains(&base64::encode(b"attached content")));
        assert!(s.find("multipart/mixed").unwrap() < s.find("multipart/alternative").unwrap());
        assert!(s.ends_with("--=_mailkit_root--\r\n"));
    }

    #[tokio::test]
    async fn inline_image_gets_content_id_and_related() {
        let png = [0x89u8, 0x50, 0x4E, 0x47];
        let msg = deterministic(
            MimeBuilder::new()
                .text_body("body")
                .html_body("<img src=\"cid:logo\">")
                .attach_inline_bytes("logo.png", "image/png", png, "logo"),
        )
        .build()
        .await
        .unwrap();
        let s = msg.as_str();
        assert!(s.contains("Content-Type: multipart/related; boundary=\"=_mailkit_root\""));
        assert!(s.contains("Content-Disposition: inline; filename=\"logo.png\""));
        assert!(s.contains("Content-ID: <logo>"));
        assert!(s.contains(&base64::encode(&png)));
        assert!(s.find("multipart/related").unwrap() < s.find("multipart/alternative").unwrap());
    }

    #[tokio::test]
    async fn non_ascii_subject_is_rfc2047_encoded() {
        let msg = deterministic(MimeBuilder::new().text_body("x").subject("Grüße"))
            .build()
            .await
            .unwrap();
        assert!(msg.as_str().contains("Subject: =?utf-8?B?"));
        let encoded = msg
            .as_str()
            .split("Subject: =?utf-8?B?")
            .nth(1)
            .unwrap()
            .split("?=")
            .next()
            .unwrap();
        assert_eq!(base64::encode("Grüße".as_bytes()), encoded);
    }

    #[tokio::test]
    async fn header_injection_is_neutralized() {
        let msg = deterministic(
            MimeBuilder::new()
                .from("a@b.com")
                .to("c@d.com\r\nBCC: victim@evil.com")
                .subject("Subject\r\nX-Evil: 1")
                .text_body("x"),
        )
        .build()
        .await
        .unwrap();
        let s = msg.as_str();
        assert!(!s.contains("victim@evil.com\r\nBCC"));
        assert!(!s.contains("X-Evil: 1\r\nMIME"));
    }

    #[tokio::test]
    async fn long_ascii_line_uses_base64() {
        let long = "a".repeat(1000);
        let msg = deterministic(MimeBuilder::new().text_body(long))
            .build()
            .await
            .unwrap();
        assert!(msg.as_str().contains("Content-Transfer-Encoding: base64"));
    }

    #[tokio::test]
    async fn no_body_produces_empty_text_part() {
        let msg = deterministic(MimeBuilder::new().from("a@b.com").to("c@d.com"))
            .build()
            .await
            .unwrap();
        assert!(
            msg.as_str()
                .contains("Content-Type: text/plain; charset=utf-8")
        );
        assert!(
            msg.as_str()
                .ends_with("Content-Transfer-Encoding: 7bit\r\n\r\n")
        );
    }

    #[tokio::test]
    async fn oversize_bytes_attachment_is_rejected() {
        let err = MimeBuilder::new()
            .text_body("x")
            .max_attachment_size(4)
            .attach_bytes("big.bin", "application/octet-stream", [0u8; 8])
            .build()
            .await
            .unwrap_err();
        assert!(err.to_string().contains("exceeding the 4 byte limit"));
    }
}
