# Changelog

All notable changes to this project are documented here. Format: [Keep a
Changelog](https://keepachangelog.com/) — versions follow [semver](https://semver.org).

## [Unreleased]

## [0.3.1] - 2026-09-12

### Added

- `tests/config_matrix.rs` — per-knob behavior matrix for all 14 message /
  MIME knobs (builder from/to/cc/bcc/subject/html/text/attachment +
  `MimeBuilder` date/message-id/extra-header/boundary/max-size/inline).
  Provider wire knobs were verified already covered in
  `tests/providers.rs`; no dead knobs found (SMTP host/port/credentials
  apply at TCP-send time; Bcc non-leak into MIME pinned).

## [0.3.0] - 2026-09-11

### Added

- **Provider abstraction v2**: `MailProvider` trait — dyn-compatible,
  returns a `SendReceipt` (`provider` + `message_id`) — implemented by every
  built-in provider. `Box<dyn MailProvider>` implements `EmailProvider`, so
  trait objects drive `EmailClient` and the retry queue unchanged.
  `EmailClient::send_with_receipt` and `EmailClient::provider` added.
- **AWS SES provider** (`ses` feature): SES v2 `SendEmail` REST-JSON API
  with hand-rolled AWS Signature Version 4 (~150 lines, no
  `aws-sdk-sesv2`/100+ dep chain) — verified against the official AWS
  known-answer test vector. Config: region, access/secret key, optional
  session token, configuration set. Attachment messages are sent as SES
  `Raw` MIME via the built-in builder.
- **SendGrid provider** (`sendgrid` feature): v3 `mail/send` —
  personalizations, custom args, categories, sandbox mode, open/click
  tracking toggles, receipt from `X-Message-Id`.
- **Postmark provider** (`postmark` feature): `/email` — message stream,
  tag, metadata, Postmark `ErrorCode` surfacing, receipt from `MessageID`.
- **MIME multipart builder** (`mime` module, always available,
  lettre-free): `multipart/mixed` + `multipart/alternative` + `related`
  (inline images with `Content-ID`), RFC 2047 encoded-words for non-ASCII
  headers, CRLF-only output with bare-LF normalization, header-injection
  sanitization, collision-checked boundary generation. Golden-fixture and
  parse-back tested.
- **Attachment streaming**: file attachments are chunk-read and
  base64-encoded incrementally with a 25 MiB (configurable) size guard;
  path-based attachments resolve at send time so queue messages can hold
  path refs.
- Examples for every provider plus a MIME builder walkthrough; README
  provider matrix.

### Fixed
- Resend attachments: `EmailMessage` attachments are now actually sent
  (base64), not silently dropped.

## [0.2.1] - 2026-09-11

### Fixed

- 22-gate quality audit pass: documentation completeness
  (README badges, REQUIREMENTS/THREAT-MODEL coverage) and
  feature-gated test hygiene.

## [0.2.0] - 2026-09-05

### Added
- Initial public release.
