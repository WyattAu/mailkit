//! Build a MIME multipart message with the self-contained builder.
//!
//! Run with: `cargo run --example mime_email`

use mailkit::mime::MimeBuilder;

#[tokio::main]
async fn main() -> Result<(), mailkit::EmailError> {
    let mime = MimeBuilder::new()
        .from("Alice <alice@example.com>")
        .to("bob@example.com")
        .cc("carol@example.com")
        .subject("Quarterly report — with charts")
        .text_body("Please see the attached report and inline chart.")
        .html_body(
            "<p>Please see the attached report and inline chart.</p>\
                    <img src=\"cid:chart\" alt=\"chart\">",
        )
        .attach_bytes("report.pdf", "application/pdf", b"%PDF-1.4 ...")
        .attach_inline_bytes(
            "chart.png",
            "image/png",
            [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
            "chart",
        )
        .build()
        .await?;

    print!("{}", mime.as_str());
    Ok(())
}
