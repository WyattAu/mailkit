//! Minimal AWS Signature Version 4 signing (feature `ses`).
//!
//! Hand-rolled rather than pulling in `aws-sdk-sesv2` (which carries 100+
//! transitive dependencies): SigV4 is small and fully specified — a canonical
//! request, a derived-key HMAC chain, and a hex signature. This module signs
//! the SES v2 `SendEmail` JSON call on the same `reqwest` client the other
//! HTTP providers use.
//!
//! Reference:
//! [AWS General Reference — Signature Version 4 signing process](https://docs.aws.amazon.com/general/latest/gr/sigv4-signing.html).
//!
//! Correctness is pinned by a known-answer test against the canonical AWS
//! documentation example (`GET iam ListUsers`).

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// Headers produced by [`sign_request`] that must be set on the HTTP call.
#[derive(Debug, Clone)]
pub(crate) struct SignedRequest {
    /// Value for the `Authorization` header.
    pub authorization: String,
    /// Value for the `x-amz-date` header (e.g. `20260911T120000Z`).
    pub amz_date: String,
}

/// SHA-256 of `data`, lowercase hex.
pub(crate) fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

/// HMAC-SHA256 of `data` under `key`, lowercase hex.
pub(crate) fn hmac_sha256_hex(key: &[u8], data: &[u8]) -> String {
    hex(&hmac_sha256(key, data))
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    // HMAC-SHA256 accepts keys of any length, so construction cannot fail.
    #[allow(clippy::expect_used)]
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC-SHA256 accepts keys of any length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// The SigV4 derived signing key:
/// `HMAC(HMAC(HMAC(HMAC("AWS4" + secret, date), region), service), "aws4_request")`.
pub(crate) fn derive_signing_key(
    secret_key: &str,
    date: &str,
    region: &str,
    service: &str,
) -> Vec<u8> {
    let k_date = hmac_sha256(format!("AWS4{secret_key}").as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    hmac_sha256(&k_service, b"aws4_request")
}

/// Build the canonical request string.
///
/// `headers` must already be lowercase-named, trimmed, and sorted by name.
fn canonical_request(
    method: &str,
    canonical_uri: &str,
    canonical_query: &str,
    headers: &[(&str, &str)],
    payload_hash: &str,
) -> String {
    let header_block: String = headers.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let signed_headers: Vec<&str> = headers.iter().map(|(k, _)| *k).collect();
    format!(
        "{method}\n{canonical_uri}\n{canonical_query}\n{header_block}\n{}\n{payload_hash}",
        signed_headers.join(";")
    )
}

/// Sign a request per SigV4 and return the headers to attach.
///
/// `headers` are additional headers to include in the signature (besides
/// `content-type`, `host`, and `x-amz-date`); names must be lowercase and
/// the slice must be sorted by name.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sign_request(
    method: &str,
    host: &str,
    canonical_uri: &str,
    canonical_query: &str,
    content_type: &str,
    extra_headers: &[(&str, &str)],
    payload: &[u8],
    amz_date: &str,
    access_key: &str,
    secret_key: &str,
    region: &str,
    service: &str,
) -> SignedRequest {
    let payload_hash = sha256_hex(payload);

    // Canonical header names, sorted: content-type, host, x-amz-date,
    // x-amz-security-token (lexicographic).
    let mut headers: Vec<(&str, String)> = vec![
        ("content-type", content_type.to_owned()),
        ("host", host.to_owned()),
        ("x-amz-date", amz_date.to_owned()),
    ];
    for (k, v) in extra_headers {
        headers.push((k, (*v).to_owned()));
    }
    headers.sort_by(|a, b| a.0.cmp(b.0));
    let header_refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();

    let creq = canonical_request(
        method,
        canonical_uri,
        canonical_query,
        &header_refs,
        &payload_hash,
    );
    let creq_hash = sha256_hex(creq.as_bytes());

    let date = &amz_date[..8.min(amz_date.len())];
    let scope = format!("{date}/{region}/{service}/aws4_request");
    let string_to_sign = format!("AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{creq_hash}");

    let key = derive_signing_key(secret_key, date, region, service);
    let signature = hmac_sha256_hex(&key, string_to_sign.as_bytes());

    let signed_names: Vec<&str> = headers.iter().map(|(k, _)| *k).collect();
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={access_key}/{scope}, SignedHeaders={}, Signature={signature}",
        signed_names.join(";")
    );

    SignedRequest {
        authorization,
        amz_date: amz_date.to_owned(),
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// AWS General Reference, "Example: computing a Signature Version 4
    /// signature" — GET `iam.amazonaws.com` `ListUsers`, credentials
    /// `AKIDEXAMPLE`, 20150830T123600Z. Reproducing the documented
    /// authorization header proves the whole chain (canonical request,
    /// string-to-sign, derived key, signature).
    #[test]
    fn sigv4_known_answer_aws_docs_vector() {
        let amz_date = "20150830T123600Z";
        let signed = sign_request(
            "GET",
            "iam.amazonaws.com",
            "/",
            "Action=ListUsers&Version=2010-05-08",
            "application/x-www-form-urlencoded; charset=utf-8",
            &[],
            b"",
            amz_date,
            "AKIDEXAMPLE",
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "us-east-1",
            "iam",
        );

        assert_eq!(
            signed.authorization,
            concat!(
                "AWS4-HMAC-SHA256 ",
                "Credential=AKIDEXAMPLE/20150830/us-east-1/iam/aws4_request, ",
                "SignedHeaders=content-type;host;x-amz-date, ",
                "Signature=5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7"
            )
        );
        assert_eq!(signed.amz_date, amz_date);
    }

    #[test]
    fn empty_payload_hash() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn signing_key_changes_with_scope() {
        let a = derive_signing_key("secret", "20260101", "us-east-1", "ses");
        let b = derive_signing_key("secret", "20260102", "us-east-1", "ses");
        let c = derive_signing_key("secret2", "20260101", "us-east-1", "ses");
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 32);
    }

    #[test]
    fn session_token_is_signed() {
        let base = sign_request(
            "POST",
            "email.us-east-1.amazonaws.com",
            "/v2/email/outbound-emails",
            "",
            "application/json",
            &[],
            b"{}",
            "20260911T000000Z",
            "AKI",
            "sk",
            "us-east-1",
            "ses",
        );
        let with_token = sign_request(
            "POST",
            "email.us-east-1.amazonaws.com",
            "/v2/email/outbound-emails",
            "",
            "application/json",
            &[("x-amz-security-token", "TOKEN")],
            b"{}",
            "20260911T000000Z",
            "AKI",
            "sk",
            "us-east-1",
            "ses",
        );
        assert!(with_token.authorization.contains("x-amz-security-token"));
        assert_ne!(base.authorization, with_token.authorization);
    }
}
