//! Send an email through the SendGrid v3 API.
//!
//! Run with: `cargo run --example sendgrid_email --features sendgrid -- SG.your_api_key`

use mailkit::{EmailClient, EmailMessage, SendGridProvider};

#[tokio::main]
async fn main() -> Result<(), mailkit::EmailError> {
    let Some(api_key) = std::env::args().nth(1) else {
        eprintln!("usage: cargo run --example sendgrid_email -- <api_key>");
        std::process::exit(2);
    };

    let provider = SendGridProvider::new(api_key)
        .with_category("mailkit-demo")
        .with_custom_arg("source", "mailkit-example")
        .with_open_tracking(false)
        .with_click_tracking(false);

    let client = EmailClient::new(provider);

    let message = EmailMessage::builder()
        .from("you@yourdomain.com")
        .to("recipient@example.com")
        .subject("Hello from mailkit (SendGrid)")
        .text_body("Sent via SendGrid v3 /v3/mail/send.")
        .html_body("<h1>Hello!</h1><p>Sent via SendGrid v3.</p>")
        .build()?;

    let receipt = client.send_with_receipt(message).await?;
    println!("SendGrid message id: {:?}", receipt.message_id);
    Ok(())
}
