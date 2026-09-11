//! Send an email through AWS SES v2 with hand-rolled SigV4 signing.
//!
//! Run with: `cargo run --example ses_email --features ses -- us-east-1 AKIA... secret you@yourdomain.com`
//!
//! Demonstrates both Simple content and Raw MIME (attachments).

use mailkit::message::Attachment;
use mailkit::{EmailClient, EmailMessage, SesProvider};

#[tokio::main]
async fn main() -> Result<(), mailkit::EmailError> {
    const USAGE: &str =
        "usage: cargo run --example ses_email -- <region> <access_key> <secret_key> [from]";
    let mut args = std::env::args().skip(1);
    let mut next = || {
        args.next().unwrap_or_else(|| {
            eprintln!("{USAGE}");
            std::process::exit(2);
        })
    };
    let region = next();
    let access_key = next();
    let secret_key = next();
    let from = args.next().unwrap_or_else(|| "you@yourdomain.com".into());

    let provider = SesProvider::new(region, access_key, secret_key, from.clone());
    let client = EmailClient::new(provider);

    let message = EmailMessage::builder()
        .from(from)
        .to("recipient@example.com")
        .subject("Hello from mailkit (SES)")
        .text_body("Signed with SigV4, no AWS SDK needed.")
        .html_body("<h1>Hello!</h1><p>Signed with SigV4, no AWS SDK needed.</p>")
        // Any attachment switches the send to SES Raw MIME automatically.
        .attachment(Attachment {
            filename: "notes.txt".into(),
            content_type: "text/plain".into(),
            path: None,
            bytes: Some(b"mailkit SES raw attachment".to_vec()),
        })
        .build()?;

    let receipt = client.send_with_receipt(message).await?;
    println!("SES message id: {:?}", receipt.message_id);
    Ok(())
}
