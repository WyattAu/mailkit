# Threat Model — mailkit

Reference: STRIDE. Scope: the crate's public API surface (`EmailClient`,
`EmailMessage` builder, `ResendProvider`, `SesProvider`, `SendGridProvider`,
`PostmarkProvider`, `SmtpProvider`/`AsyncSmtpProvider`, `MimeBuilder`,
`EmailQueue`, `AuditLogger`, `thread_messages`) as used by a downstream
service. Trust boundaries: (1) strings entering the message builder
(addresses, subject, bodies), (2) provider API credentials (API keys, AWS
access/secret keys, SMTP passwords), (3) the audit
log contents, (4) the dependency tree (reqwest for HTTP providers, lettre
for SMTP, sha2/hmac for SigV4).

## Assets

| ID | Asset | Example |
|----|-------|---------|
| A1 | Integrity of outbound email (recipient and headers as intended) | CRLF injection turns a subject into extra SMTP recipients |
| A2 | Confidentiality of the provider API key / SMTP password | Credential rendered via `Debug` or error text |
| A3 | PII of message participants (addresses, subjects) | Audit log or errors exposing recipient lists |

## STRIDE Analysis

| # | Threat | Category | Surface | Mitigation | Verifying test |
|---|--------|----------|---------|------------|----------------|
| T1 | Header/recipient injection (CRLF in subject, addresses, bodies) | Spoofing/Tampering | `EmailMessage::builder`, `SmtpProvider` | **Not mitigated** — addresses and subject are plain `String`s; the builder only checks non-empty from/≥1 recipient, with no CRLF or control-character rejection. Documented residual risk: sanitize inputs before the builder | `builder_missing_from_fails`, `builder_missing_recipient_fails` (validation that *does* exist); no injection tests exist |
| T2 | Recipient address format abuse (Resend API) | Tampering | `ResendProvider` | Resend path rejects addresses that fail its parse (`invalid recipient address`) — minimal format gate, not an injection defense | Resend provider error path (`src/provider.rs:123`); no dedicated integration test — documented |
| T3 | API key / SMTP credential leakage via `Debug` or errors | Info disclosure | `ResendProvider`, `SmtpProvider`, `AsyncSmtpProvider` | Provider structs do **not** derive `Debug`; the key lives in a private `String` and is only placed into the `Authorization` header at send time. No method returns it | Code review; no `Debug` derive on provider structs (`src/provider.rs`); `error_provider_display` shows errors carry caller-provided text only |
| T4 | Message content leakage via `Debug`/logs | Info disclosure | `EmailMessage` | **Not mitigated** — `EmailMessage` derives `Debug` including all addresses, subject, and bodies. Documented residual risk: never log messages raw | `derive(Debug)` at `src/message.rs:6` (code review) |
| T5 | Audit-trail gaps (send denial / unlogged sends) | Repudiation | `EmailClient::send`, `AuditLogger` | **Partially mitigated, opt-in**: an `AuditLogger` trait + `InMemoryAuditLog` exist with typed `EmailLogEntry`s, but `EmailClient::send` does **not** wire logging automatically — callers must call `log()` themselves | `in_memory_audit_log_append_and_query`, `audit_log_append_multiple_entries`, `log_entry_failed_status_with_error` (log correctness, not wiring) |
| T6 | Unbounded queue/audit growth | DoS | `EmailQueue`, `InMemoryAuditLog` | **Not mitigated** — both grow without bound; `queue.process` drains but nothing caps enqueue rate or log size. Documented residual risk | `queue_enqueue_and_len`, `queue_process_drains_queue` (behavior only) |
| T7 | Mis-threading (messages attached to wrong conversation) | Tampering | `thread_messages` | Deterministic JWZ-lite grouping (normalized subject window + reply-chain anchors); unanchored messages get unique threads rather than being force-grouped | `threading_is_deterministic`, `root_thread_key`, `unanchored_messages_get_unique_threads` |

## Out of Scope

- SPF/DKIM/DMARC: message authenticity in receivers' eyes is the sending
  domain's mail infrastructure, not this client.
- Recipient address *semantic* validity (does the mailbox exist) —
  provider-side.
- Transport security to providers (HTTPS/STARTTLS policy lives in reqwest /
  lettre configs).

## Residual Risks

- **R1 (High, accepted for now):** No CRLF/control-character rejection in
  the message builder (T1). A caller that interpolates user input into
  subject or addresses exposes SMTP header injection. This is the crate's
  most significant gap; treat builder input as untrusted upstream until a
  validation pass lands.
- **R2 (Medium, accepted):** Audit logging is opt-in and unenforced (T5) —
  a send can complete with no record, defeating repudiation protection.
  Wire the audit hook into `EmailClient` when compliance requires it.
- **R3 (Low, accepted):** `InMemoryAuditLog` stores full PII (addresses,
  subjects) unencrypted and unbounded (T6, T4).
- **R4 (Low, accepted):** Dependency risk in lettre/reqwest; no in-repo
  `cargo audit` gate (org-level Dependabot only).
