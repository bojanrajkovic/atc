# CLAUDE.md -- atc-github

Last verified: 2026-04-11

> Canonical documentation lives in `docs/architecture/backend-server.md` (GitHub API Integration section). This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

GitHub webhook parsing and HMAC-SHA256 signature verification for ATC. Translates raw GitHub webhook payloads into `atc-core` domain events. This crate is the boundary between GitHub's JSON API and the source-agnostic domain model.

## Modules

| Module | Role |
|--------|------|
| `webhook/mod.rs` | Public API: `parse_webhook`, `ParseError`, `ParseResult`, `WebhookEvent` |
| `webhook/verify.rs` | `verify_signature`, `VerifyError` -- HMAC-SHA256 constant-time verification |
| `webhook/types.rs` | `pub(crate)` serde structs for GitHub webhook JSON payloads |
| `webhook/translate.rs` | `pub(crate)` translation from serde types to `atc-core` domain events |

## Contracts

These rules are enforced by the implementation and verified by 42 tests:

- **Two public entry points:** `verify_signature` and `parse_webhook`. All other types are `pub(crate)` or private.
- **Three-way parse result:** `parse_webhook` returns `ParseResult::Parsed` for `workflow_run` and `workflow_job` events, `ParseResult::Skipped` for all other event types. Errors are reserved for actual failures (malformed JSON, unknown actions/conclusions).
- **Opaque payload types:** Consumers never see GitHub-specific serde types. The public API accepts `&[u8]` body bytes and `&str` event type, returning `atc-core` domain events.
- **Structured error context:** Every `ParseError` variant carries enough context (event type, action, value) for observability without the raw payload.
- **SHA-256 only:** `verify_signature` accepts `sha256=<hex>` signatures. SHA-1 is explicitly rejected (`RejectedAlgorithm`), unknown algorithms return `UnknownAlgorithm`.
- **Constant-time comparison:** Signature verification uses HMAC's `verify_slice` for timing-safe comparison.
- **Forward compatibility:** Serde types use default deny-unknown-fields=false, so new GitHub payload fields are silently ignored.

## Dependencies

- **Uses:** `atc-core` (domain types: `RunEvent`, `JobEvent`, envelope structs, `RunnerInfo`, `Step`, `StepStatus`, conclusions, IDs)
- **Used by:** `atc-server` (future: webhook route handler will call `verify_signature` then `parse_webhook`)
- **External:** `hmac`/`sha2` for HMAC-SHA256, `const-hex` for hex decoding, `serde`/`serde_json` for JSON, `chrono` for timestamps, `thiserror` for error types

## Testing

```bash
cargo test -p atc-github        # 42 tests
cargo clippy -p atc-github -- -D warnings
```

Test fixtures in `tests/fixtures/` are curated from real CI webhook captures covering all 7 event type/action combinations.

## Key References

- Architecture: `docs/architecture/backend-server.md` section "GitHub API Integration"
- Design plan: `docs/design-plans/2026-04-11-gh-webhooks.md`
