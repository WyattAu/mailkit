# mailkit

Email delivery for Rust — SMTP (lettre) and Resend providers with template rendering, queue, and audit logging.

## Features

- **Resend API** — HTTP-based email delivery via [Resend](https://resend.com) (default)
- **SMTP** — Direct SMTP transport via [lettre](https://docs.rs/lettre)
- **Templates** — Server-side template rendering via [Tera](https://docs.rs/tera)
- **Queue** — Async retry queue for reliable delivery
- **Audit logging** — Track sent/failed messages with pluggable loggers

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

## Comparison with raw libraries

| Feature | mailkit | raw lettre/reqwest |
|---|---|---|
| Provider abstraction | Yes | No |
| Retry queue | Built-in | Manual |
| Audit logging | Built-in | Manual |
| Template rendering | Via `templates` feature | Separate crate |
| Type-safe builder | Yes | No |

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
