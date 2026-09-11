# Requirements — mailkit

Numbered, testable requirements. Every requirement maps to at least one named
test or doc-comment contract; security-relevant items cite THREAT-MODEL.md rows.

Scope: Email toolkit — MIME composition, SMTP/Resend/SES/SendGrid/Postmark providers, SigV4 signing, attachment streaming, thread detection

## Functional

| ID | Requirement | Priority |
|----|-------------|----------|
| REQ-MK-001 | Composed MIME messages parse back to the same headers/body (roundtrip) | MUST |
| REQ-MK-004 | MIME output is byte-stable given pinned date/message-id/boundaries (golden fixtures) | MUST |
| REQ-MK-005 | Attachment streaming never buffers raw file bytes wholly and enforces the configured size guard | MUST |
| REQ-MK-006 | SigV4 signatures reproduce the official AWS known-answer vector | MUST |
| REQ-MK-002 | Address parsing rejects malformed addresses with typed errors | MUST |
| REQ-MK-003 | Thread detection groups messages by normalized subject + References/Message-ID windowing | MUST |

## Security

| ID | Requirement | Priority |
|----|-------------|----------|
| REQ-MK-100 | Header values are encoded/escaped; CRLF injection in headers is impossible through the typed builder | MUST |
| REQ-MK-101 | HTML bodies are composed by the caller; the crate performs no unsanitized passthrough of untrusted markup into text parts | SHOULD |

## Observability & API hygiene

| ID | Requirement | Priority |
|----|-------------|----------|
| REQ-MK-900 | All fallible public APIs return typed errors; production `unwrap`/`expect` is denied or explicitly justified with an invariant comment | MUST |
| REQ-MK-901 | Public items carry doc comments with runnable examples where practical | SHOULD |

Reviewed: 2026-09-11
