//! Send an email through the Postmark API.
//!
//! Run with: `cargo run --example postmark_email --features postmark -- your_server_token`

use mailkit::{EmailClient, EmailMessage, PostmarkProvider};

#[tokio::main]
async fn main() -> Result<(), mailkit::EmailError> {
    let Some(server_token) = std::env::args().nth(1) else {
        eprintln!("usage: cargo run --example postmark_email -- <server_token>");
        std::process::exit(2);
    };

    let provider = PostmarkProvider::new(server_token)
        .with_message_stream("outbound")
        .with_tag("mailkit-demo")
        .with_metadata("source", "mailkit-example");

    let client = EmailClient::new(provider);

    let message = EmailMessage::builder()
        .from("you@yourdomain.com")
        .to("recipient@example.com")
        .subject("Hello from mailkit (Postmark)")
        .text_body("Sent via Postmark /email.")
        .html_body("<h1>Hello!</h1><p>Sent via Postmark.</p>")
        .build()?;

    let receipt = client.send_with_receipt(message).await?;
    println!("Postmark message id: {:?}", receipt.message_id);
    Ok(())
}
