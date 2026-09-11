//! Send an email through SMTP via lettre.
//!
//! Run with: `cargo run --example smtp_email --features smtp -- smtp.example.com 587 user pass you@yourdomain.com`

use mailkit::{EmailClient, EmailMessage, SmtpProvider};

#[tokio::main]
async fn main() -> Result<(), mailkit::EmailError> {
    const USAGE: &str =
        "usage: cargo run --example smtp_email -- <host> <port> <user> <pass> <from>";
    let mut args = std::env::args().skip(1);
    let Some(host) = args.next() else {
        eprintln!("{USAGE}");
        std::process::exit(2);
    };
    let Some(port) = args.next().and_then(|p| p.parse::<u16>().ok()) else {
        eprintln!("{USAGE}");
        std::process::exit(2);
    };
    let username = args.next();
    let password = args.next();
    let Some(from) = args.next().and_then(|f| f.parse().ok()) else {
        eprintln!("{USAGE}");
        std::process::exit(2);
    };

    let provider = SmtpProvider::new(host, port, username, password, from)?;
    let client = EmailClient::new(provider);

    let message = EmailMessage::builder()
        .from("you@yourdomain.com")
        .to("recipient@example.com")
        .subject("Hello from mailkit (SMTP)")
        .text_body("Sent via lettre SMTP transport.")
        .build()?;

    client.send(message).await?;
    println!("SMTP send accepted");
    Ok(())
}
