// Integration tests: unwrap is acceptable for test assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Provider request-shape tests against a local HTTP server (wiremock).
//!
//! Each provider test asserts the request path, auth headers, and JSON body
//! shape, plus receipt parsing from the provider's response.

use std::future::Future;
use std::pin::Pin;

use mailkit::message::EmailMessage;
use mailkit::provider::{MailProvider, SendReceipt};
use mailkit::{EmailClient, EmailError};

#[cfg(any(
    feature = "resend",
    feature = "ses",
    feature = "sendgrid",
    feature = "postmark"
))]
mod http_support {
    use mailkit::message::{Attachment, EmailMessage};

    pub use wiremock::matchers::{body_partial_json, header, method, path};
    pub use wiremock::{Mock, MockServer, ResponseTemplate};

    pub fn sample_message() -> EmailMessage {
        EmailMessage::builder()
            .from("sender@example.com")
            .to("to1@example.com")
            .to("to2@example.com")
            .cc("cc@example.com")
            .bcc("bcc@example.com")
            .subject("Shape test")
            .text_body("plain text")
            .html_body("<p>html text</p>")
            .build()
            .unwrap()
    }

    pub fn test_attachment() -> Attachment {
        Attachment {
            filename: "data.bin".into(),
            content_type: "application/octet-stream".into(),
            path: None,
            bytes: Some(vec![0xFA, 0xCE, 0xB0, 0x0C]),
        }
    }

    /// Independent base64 decoder for verifying encoded payloads in requests.
    pub fn b64_decode(input: &str) -> Vec<u8> {
        const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        let mut acc: u32 = 0;
        let mut bits = 0u32;
        for c in input.chars().filter(|c| !c.is_whitespace()) {
            if c == '=' {
                break;
            }
            let v = ALPHABET.find(c).expect("valid base64 char") as u32;
            acc = (acc << 6) | v;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push(((acc >> bits) & 0xFF) as u8);
            }
        }
        out
    }

    pub async fn captured_body(server: &MockServer) -> serde_json::Value {
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        serde_json::from_slice(&requests[0].body).unwrap()
    }
}

#[cfg(any(
    feature = "resend",
    feature = "ses",
    feature = "sendgrid",
    feature = "postmark"
))]
pub use http_support::*;

// ---------------------------------------------------------------------------
// Resend
// ---------------------------------------------------------------------------

#[cfg(feature = "resend")]
mod resend_tests {
    use super::*;
    use mailkit::ResendProvider;

    #[tokio::test]
    async fn resend_request_shape_and_receipt() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/emails"))
            .and(header("Authorization", "Bearer re_test_key"))
            .and(body_partial_json(serde_json::json!({
                "from": "sender@example.com",
                "subject": "Shape test",
                "text": "plain text",
                "html": "<p>html text</p>",
                "cc": ["cc@example.com"],
                "bcc": ["bcc@example.com"],
            })))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "resend-42"})),
            )
            .mount(&server)
            .await;

        let provider = ResendProvider::new("re_test_key").with_base_url(server.uri());
        let receipt = provider.send(&sample_message()).await.unwrap();

        assert_eq!(receipt.provider, "resend");
        assert_eq!(receipt.message_id.as_deref(), Some("resend-42"));
        let body = captured_body(&server).await;
        let to = body["to"].as_array().unwrap();
        assert_eq!(to.len(), 2);
    }

    #[tokio::test]
    async fn resend_attachments_are_base64() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "x"})))
            .mount(&server)
            .await;

        let message = EmailMessage::builder()
            .from("a@b.com")
            .to("c@d.com")
            .subject("att")
            .text_body("body")
            .attachment(test_attachment())
            .build()
            .unwrap();

        let provider = ResendProvider::new("re_test_key").with_base_url(server.uri());
        provider.send(&message).await.unwrap();

        let body = captured_body(&server).await;
        let atts = body["attachments"].as_array().unwrap();
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0]["filename"], "data.bin");
        assert_eq!(atts[0]["content_type"], "application/octet-stream");
        assert_eq!(
            b64_decode(atts[0]["content"].as_str().unwrap()),
            vec![0xFA, 0xCE, 0xB0, 0x0C]
        );
    }

    #[tokio::test]
    async fn resend_http_error_is_provider_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(422).set_body_string("validation failed"))
            .mount(&server)
            .await;

        let provider = ResendProvider::new("re_test_key").with_base_url(server.uri());
        let err = provider.send(&sample_message()).await.unwrap_err();
        assert!(err.to_string().contains("422"));
        assert!(err.to_string().contains("validation failed"));
    }

    /// `Box<dyn MailProvider>` (trait object) drives the real HTTP provider.
    #[tokio::test]
    async fn resend_via_dyn_mail_provider() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "dyn"})),
            )
            .mount(&server)
            .await;

        let provider: Box<dyn MailProvider> =
            Box::new(ResendProvider::new("re_test_key").with_base_url(server.uri()));
        assert_eq!(MailProvider::name(provider.as_ref()), "resend");

        let client = EmailClient::new(provider);
        let receipt = client.send_with_receipt(sample_message()).await.unwrap();
        assert_eq!(receipt.message_id.as_deref(), Some("dyn"));
    }
}

// ---------------------------------------------------------------------------
// SES
// ---------------------------------------------------------------------------

#[cfg(feature = "ses")]
mod ses_tests {
    use super::*;
    use mailkit::SesProvider;

    fn provider(server: &MockServer) -> SesProvider {
        SesProvider::new("us-east-1", "AKIDEXAMPLE", "secret", "sender@example.com")
            .with_endpoint(server.uri())
    }

    #[tokio::test]
    async fn ses_simple_content_request_shape() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v2/email/outbound-emails"))
            .and(body_partial_json(serde_json::json!({
                "FromEmailAddress": "sender@example.com",
                "Destination": {
                    "ToAddresses": ["to1@example.com", "to2@example.com"],
                    "CcAddresses": ["cc@example.com"],
                    "BccAddresses": ["bcc@example.com"],
                },
                "Content": {
                    "Simple": {
                        "Subject": {"Data": "Shape test", "Charset": "UTF-8"},
                        "Body": {
                            "Text": {"Data": "plain text"},
                            "Html": {"Data": "<p>html text</p>"},
                        },
                    }
                }
            })))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"MessageId": "ses-7"})),
            )
            .mount(&server)
            .await;

        let receipt = provider(&server).send(&sample_message()).await.unwrap();
        assert_eq!(receipt.provider, "ses");
        assert_eq!(receipt.message_id.as_deref(), Some("ses-7"));

        // SigV4 headers: scope pins region, service, and date.
        let requests = server.received_requests().await.unwrap();
        let headers = &requests[0].headers;
        let auth = headers.get("Authorization").unwrap().to_str().unwrap();
        assert!(auth.starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/"));
        assert!(auth.contains("/us-east-1/ses/aws4_request, "));
        assert!(auth.contains("SignedHeaders=content-type;host;x-amz-date, Signature="));
        assert!(headers.contains_key("x-amz-date"));
        assert!(!headers.contains_key("x-amz-security-token"));
    }

    #[tokio::test]
    async fn ses_session_token_is_sent_and_signed() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("x-amz-security-token", "STS-TOKEN"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"MessageId": "t"})),
            )
            .mount(&server)
            .await;

        let p = provider(&server).with_session_token("STS-TOKEN");
        p.send(&sample_message()).await.unwrap();

        let requests = server.received_requests().await.unwrap();
        let auth = requests[0]
            .headers
            .get("Authorization")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(auth.contains("x-amz-security-token"));
    }

    #[tokio::test]
    async fn ses_configuration_set_is_sent() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_partial_json(
                serde_json::json!({"ConfigurationSetName": "events"}),
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"MessageId": "c"})),
            )
            .mount(&server)
            .await;

        let p = provider(&server).with_configuration_set("events");
        p.send(&sample_message()).await.unwrap();
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn ses_attachments_switch_to_raw_mime() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"MessageId": "raw-1"})),
            )
            .mount(&server)
            .await;

        let message = EmailMessage::builder()
            .from("sender@example.com")
            .to("to1@example.com")
            .subject("Raw")
            .text_body("body")
            .attachment(test_attachment())
            .build()
            .unwrap();

        let receipt = provider(&server).send(&message).await.unwrap();
        assert_eq!(receipt.message_id.as_deref(), Some("raw-1"));

        let body = captured_body(&server).await;
        // Simple content must be absent; Raw carries base64 MIME.
        assert!(body["Content"].get("Simple").is_none());
        let data = body["Content"]["Raw"]["Data"].as_str().unwrap();
        let mime = String::from_utf8(b64_decode(data)).unwrap();

        // Decoded MIME is the builder's output for this message (modulo the
        // random boundary): the attachment part must be present.
        assert!(mime.contains("Content-Type: multipart/mixed; boundary=\"="));
        assert!(mime.contains("Content-Disposition: attachment; filename=\"data.bin\""));
        assert!(mime.contains("Subject: Raw\r\n"));
    }

    #[tokio::test]
    async fn ses_error_response_is_provider_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_string(r#"{"type":"AccessDenied","message":"not verified"}"#),
            )
            .mount(&server)
            .await;

        let err = provider(&server).send(&sample_message()).await.unwrap_err();
        assert!(err.to_string().contains("403"));
        assert!(err.to_string().contains("AccessDenied"));
    }
}

// ---------------------------------------------------------------------------
// SendGrid
// ---------------------------------------------------------------------------

#[cfg(feature = "sendgrid")]
mod sendgrid_tests {
    use super::*;
    use mailkit::SendGridProvider;

    fn provider(server: &MockServer) -> SendGridProvider {
        SendGridProvider::new("SG.test").with_base_url(server.uri())
    }

    #[tokio::test]
    async fn sendgrid_request_shape_and_receipt() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v3/mail/send"))
            .and(header("Authorization", "Bearer SG.test"))
            .and(body_partial_json(serde_json::json!({
                "personalizations": [{
                    "to": [{"email": "to1@example.com"}, {"email": "to2@example.com"}],
                    "cc": [{"email": "cc@example.com"}],
                    "bcc": [{"email": "bcc@example.com"}],
                }],
                "from": {"email": "sender@example.com"},
                "subject": "Shape test",
                "content": [
                    {"type": "text/plain", "value": "plain text"},
                    {"type": "text/html", "value": "<p>html text</p>"},
                ],
            })))
            .respond_with(
                ResponseTemplate::new(202)
                    .insert_header("X-Message-Id", "sg-9")
                    .set_body_string(""),
            )
            .mount(&server)
            .await;

        let receipt = provider(&server).send(&sample_message()).await.unwrap();
        assert_eq!(receipt.provider, "sendgrid");
        assert_eq!(receipt.message_id.as_deref(), Some("sg-9"));
    }

    #[tokio::test]
    async fn sendgrid_options_land_in_request() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_partial_json(serde_json::json!({
                "personalizations": [{
                    "custom_args": {"tenant": "acme", "source": "test"},
                }],
                "categories": ["marketing", "digest"],
                "mail_settings": {"sandbox_mode": {"enable": true}},
                "tracking_settings": {
                    "open_tracking": {"enable": false},
                    "click_tracking": {"enable": true},
                },
            })))
            .respond_with(ResponseTemplate::new(202).set_body_string(""))
            .mount(&server)
            .await;

        let p = provider(&server)
            .with_category("marketing")
            .with_category("digest")
            .with_custom_arg("tenant", "acme")
            .with_custom_arg("source", "test")
            .with_sandbox_mode(true)
            .with_open_tracking(false)
            .with_click_tracking(true);

        p.send(&sample_message()).await.unwrap();
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn sendgrid_defaults_omit_unset_sections() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(202).set_body_string(""))
            .mount(&server)
            .await;

        provider(&server).send(&sample_message()).await.unwrap();
        let body = captured_body(&server).await;
        assert!(body.get("mail_settings").is_none());
        assert!(body.get("tracking_settings").is_none());
        assert!(body.get("categories").is_none());
        assert!(body["personalizations"][0].get("custom_args").is_none());
    }

    #[tokio::test]
    async fn sendgrid_attachments_are_base64() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(202).set_body_string(""))
            .mount(&server)
            .await;

        let message = EmailMessage::builder()
            .from("a@b.com")
            .to("c@d.com")
            .subject("att")
            .text_body("body")
            .attachment(test_attachment())
            .build()
            .unwrap();

        provider(&server).send(&message).await.unwrap();
        let body = captured_body(&server).await;
        let atts = body["attachments"].as_array().unwrap();
        assert_eq!(atts[0]["filename"], "data.bin");
        assert_eq!(atts[0]["type"], "application/octet-stream");
        assert_eq!(atts[0]["disposition"], "attachment");
        assert_eq!(
            b64_decode(atts[0]["content"].as_str().unwrap()),
            vec![0xFA, 0xCE, 0xB0, 0x0C]
        );
    }

    #[tokio::test]
    async fn sendgrid_requires_a_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(202))
            .mount(&server)
            .await;

        let message = EmailMessage::builder()
            .from("a@b.com")
            .to("c@d.com")
            .subject("no body")
            .build()
            .unwrap();
        let err = provider(&server).send(&message).await.unwrap_err();
        assert!(err.to_string().contains("text_body or html_body"));
    }
}

// ---------------------------------------------------------------------------
// Postmark
// ---------------------------------------------------------------------------

#[cfg(feature = "postmark")]
mod postmark_tests {
    use super::*;

    fn provider(server_uri: String) -> mailkit::PostmarkProvider {
        mailkit::PostmarkProvider::new("pm-token").with_base_url(server_uri)
    }

    #[tokio::test]
    async fn postmark_request_shape_and_receipt() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/email"))
            .and(header("X-Postmark-Server-Token", "pm-token"))
            .and(header("Accept", "application/json"))
            .and(body_partial_json(serde_json::json!({
                "From": "sender@example.com",
                "To": "to1@example.com, to2@example.com",
                "Cc": "cc@example.com",
                "Bcc": "bcc@example.com",
                "Subject": "Shape test",
                "TextBody": "plain text",
                "HtmlBody": "<p>html text</p>",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "To": "to1@example.com",
                "SubmittedAt": "2026-09-11T12:00:00Z",
                "MessageID": "pm-11",
                "ErrorCode": 0,
                "Message": "OK",
            })))
            .mount(&server)
            .await;

        let receipt = provider(server.uri())
            .send(&sample_message())
            .await
            .unwrap();
        assert_eq!(receipt.provider, "postmark");
        assert_eq!(receipt.message_id.as_deref(), Some("pm-11"));
    }

    #[tokio::test]
    async fn postmark_stream_tag_metadata() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_partial_json(serde_json::json!({
                "MessageStream": "broadcasts",
                "Tag": "weekly",
                "Metadata": {"tenant": "acme"},
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "MessageID": "pm-12", "ErrorCode": 0, "Message": "OK",
            })))
            .mount(&server)
            .await;

        let p = provider(server.uri())
            .with_message_stream("broadcasts")
            .with_tag("weekly")
            .with_metadata("tenant", "acme");
        p.send(&sample_message()).await.unwrap();

        // Default stream shows up when not overridden.
        let body = captured_body(&server).await;
        assert_eq!(body["MessageStream"], "broadcasts");
    }

    #[tokio::test]
    async fn postmark_default_stream_is_outbound() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_partial_json(
                serde_json::json!({"MessageStream": "outbound"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "MessageID": "pm-13", "ErrorCode": 0, "Message": "OK",
            })))
            .mount(&server)
            .await;
        provider(server.uri())
            .send(&sample_message())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn postmark_attachments_are_base64() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "MessageID": "pm-14", "ErrorCode": 0, "Message": "OK",
            })))
            .mount(&server)
            .await;

        let message = EmailMessage::builder()
            .from("a@b.com")
            .to("c@d.com")
            .subject("att")
            .text_body("body")
            .attachment(test_attachment())
            .build()
            .unwrap();

        provider(server.uri()).send(&message).await.unwrap();
        let body = captured_body(&server).await;
        let atts = body["Attachments"].as_array().unwrap();
        assert_eq!(atts[0]["Name"], "data.bin");
        assert_eq!(atts[0]["ContentType"], "application/octet-stream");
        assert_eq!(
            b64_decode(atts[0]["Content"].as_str().unwrap()),
            vec![0xFA, 0xCE, 0xB0, 0x0C]
        );
    }

    #[tokio::test]
    async fn postmark_error_code_is_surfaced() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(422).set_body_json(serde_json::json!({
                "ErrorCode": 401,
                "Message": "Invalid API request: no recipients",
            })))
            .mount(&server)
            .await;

        let err = provider(server.uri())
            .send(&sample_message())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("postmark error 401"));
        assert!(err.to_string().contains("no recipients"));
    }
}

// ---------------------------------------------------------------------------
// Cross-provider plumbing
// ---------------------------------------------------------------------------

/// A stub [`MailProvider`] proving user-implemented dyn providers work with
/// the client, queue, and receipts.
struct RecordingProvider {
    fail_first: std::sync::atomic::AtomicBool,
}

impl MailProvider for RecordingProvider {
    fn send<'a>(
        &'a self,
        message: &'a EmailMessage,
    ) -> Pin<Box<dyn Future<Output = Result<SendReceipt, EmailError>> + Send + 'a>> {
        Box::pin(async move {
            if self
                .fail_first
                .compare_exchange(
                    true,
                    false,
                    std::sync::atomic::Ordering::SeqCst,
                    std::sync::atomic::Ordering::SeqCst,
                )
                .is_ok()
            {
                return Err(EmailError::provider("first attempt always fails"));
            }
            Ok(SendReceipt::new("recording", Some(message.subject.clone())))
        })
    }

    fn name(&self) -> &str {
        "recording"
    }
}

#[tokio::test]
async fn dyn_provider_retries_through_queue() {
    let provider: Box<dyn MailProvider> = Box::new(RecordingProvider {
        fail_first: std::sync::atomic::AtomicBool::new(true),
    });
    let client = EmailClient::new(provider);

    let message = EmailMessage::builder()
        .from("a@b.com")
        .to("c@d.com")
        .subject("queued-dyn")
        .build()
        .unwrap();
    client.queue().enqueue(message).await;

    // First process pass: fails once, re-queued.
    client.queue().process(client.provider()).await.unwrap();
    assert_eq!(client.queue().len().await, 1);

    // Second pass: succeeds and drains.
    client.queue().process(client.provider()).await.unwrap();
    assert!(client.queue().is_empty().await);
}
