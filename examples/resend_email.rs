//! Send an email through the Resend API.
//!
//! Run with: `cargo run --example resend_email -- re_your_api_key`

use mailkit::{EmailClient, EmailMessage, ResendProvider};

#[tokio::main]
async fn main() -> Result<(), mailkit::EmailError> {
    let Some(api_key) = std::env::args().nth(1) else {
        eprintln!("usage: cargo run --example resend_email -- <api_key>");
        std::process::exit(2);
    };

    let client = EmailClient::new(ResendProvider::new(api_key));

    let message = EmailMessage::builder()
        .from("you@yourdomain.com")
        .to("recipient@example.com")
        .subject("Hello from mailkit")
        .text_body("Plain text fallback.")
        .html_body("<h1>Hello!</h1><p>Sent via <b>mailkit</b> + Resend.</p>")
        .build()?;

    let receipt = client.send_with_receipt(message).await?;
    println!("Resend message id: {:?}", receipt.message_id);
    Ok(())
}
