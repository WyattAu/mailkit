# mailkit

[![docs.rs](https://docs.rs/mailkit/badge.svg)](https://docs.rs/mailkit)
[![crates.io](https://img.shields.io/crates/v/mailkit.svg)](https://crates.io/crates/mailkit)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

Email delivery for Rust — SMTP, Resend, AWS SES, SendGrid, and Postmark
providers with MIME multipart building, queue, and audit logging.

## Provider matrix

| Provider | Feature | Default | Transport | Attachments | Receipts |
|---|---|---|---|---|---|
| Resend | `resend` | ✅ | HTTPS JSON (`POST /emails`) | base64 (paths streamed) | `id` |
| SMTP (lettre) | `smtp` | — | SMTP (sync + async) | via lettre | — |
| AWS SES v2 | `ses` | — | HTTPS JSON + hand-rolled SigV4 (no AWS SDK) | Raw MIME via built-in builder | `MessageId` |
| SendGrid v3 | `sendgrid` | — | HTTPS JSON (`POST /v3/mail/send`) | base64 (paths streamed) | `X-Message-Id` |
| Postmark | `postmark` | — | HTTPS JSON (`POST /email`) | base64 (paths streamed) | `MessageID` |
| MIME builder | — (always available) | ✅ | in-process | streaming + size guard | — |

All providers implement `EmailProvider` (0.2.x fire-and-forget) **and**
`MailProvider` (0.3.0: returns a `SendReceipt`, dyn-compatible, so providers
compose as `Box<dyn MailProvider>`).

## Usage

```rust
use mailkit::{EmailClient, EmailMessage, ResendProvider};

#[tokio::main]
async fn main() -> Result<(), mailkit::EmailError> {
    let provider = ResendProvider::new("re_your_api_key");
    let client = EmailClient::new(provider);

    let msg = EmailMessage::builder()
        .from("sender@example.com")
        .to("recipient@example.com")
        .subject("Hello from mailkit")
        .html_body("<h1>Hello!</h1><p>This is mailkit.</p>")
        .text_body("Hello! This is mailkit.")
        .build()?;

    client.send(msg).await?;
    Ok(())
}
```

### AWS SES (SigV4 by hand — no AWS SDK dependency)

The `ses` feature signs SES v2 `SendEmail` requests with a hand-rolled
AWS Signature Version 4 implementation (~150 lines, verified against the
official AWS known-answer test vector) on the same `reqwest` client the
other HTTP providers use. Messages with attachments are rendered by the
built-in MIME builder and sent as SES `Raw` content.

```rust
use mailkit::{EmailClient, EmailMessage, SesProvider};

let provider = SesProvider::new("us-east-1", "AKIA...", "secret", "sender@example.com")
    .with_session_token("optional-temporary-credentials-token")
    .with_configuration_set("my-config-set");
let client = EmailClient::new(provider);

let receipt = client.send_with_receipt(msg).await?;
println!("SES message id: {:?}", receipt.message_id);
```

### SendGrid

```rust
use mailkit::{EmailClient, SendGridProvider};

let provider = SendGridProvider::new("SG.your-api-key")
    .with_category("transactional")
    .with_custom_arg("tenant", "acme")
    .with_sandbox_mode(false)
    .with_open_tracking(true)
    .with_click_tracking(false);
let client = EmailClient::new(provider);
```

### Postmark

```rust
use mailkit::{EmailClient, PostmarkProvider};

let provider = PostmarkProvider::new("your-server-token")
    .with_message_stream("outbound")
    .with_tag("weekly-digest")
    .with_metadata("tenant", "acme");
let client = EmailClient::new(provider);
```

### Trait objects and receipts

```rust
use mailkit::provider::MailProvider;
use mailkit::SendGridProvider;

let provider: Box<dyn MailProvider> = Box::new(SendGridProvider::new("SG..."));
let client = EmailClient::new(provider);          // EmailProvider impl on Box<dyn MailProvider>
let receipt = client.send_with_receipt(msg).await?; // SendReceipt { provider, message_id }
```

## MIME multipart builder

The `mime` module is self-contained (no lettre dependency): text + HTML
(`multipart/alternative`), inline images with `Content-ID`
(`multipart/related`), and attachments (`multipart/mixed`), with RFC 2047
encoded-words for non-ASCII headers, 76-column-folded base64 transfer
encoding, CRLF line endings, and header-injection sanitization.

```rust
use mailkit::mime::MimeBuilder;

let mime = MimeBuilder::new()
    .from("Alice <alice@example.com>")
    .to("bob@example.com")
    .subject("Report")
    .text_body("See attached.")
    .html_body("<p>See <b>attached</b>.</p><img src=\"cid:chart\">")
    .attach_inline_bytes("chart.png", "image/png", png_bytes, "chart")
    .build()
    .await?;
```

### Attachment streaming

File attachments are read in chunks and base64-encoded incrementally (raw
bytes are never fully buffered), with a per-attachment size guard
(`max_attachment_size`, default 25 MiB). Path-based attachments are read at
send time, so `EmailMessage`s holding path references can safely sit in the
`EmailQueue`.

## Queue & audit

```rust
use mailkit::audit::{AuditLogger, InMemoryAuditLog};

client.queue().enqueue(msg).await;
client.queue().process(client.provider()).await?; // retries up to 3 attempts
```

## Comparison with raw libraries

| Feature | mailkit | raw lettre/reqwest |
|---|---|---|
| Provider abstraction (5 providers) | Yes | No |
| Send receipts / dyn-compatible trait | Yes | No |
| SigV4 signing without AWS SDK | Yes | Manual |
| MIME multipart builder | Built-in | lettre's (heavier) |
| Streaming attachment encoding | Built-in | Manual |
| Retry queue | Built-in | Manual |
| Audit logging | Built-in | Manual |
| Type-safe builder | Yes | No |

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
