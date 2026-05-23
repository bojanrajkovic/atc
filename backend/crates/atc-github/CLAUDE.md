# CLAUDE.md — atc-github

Last verified: 2026-05-23

> Canonical documentation lives in `docs/architecture/backend-server.md` (GitHub API Integration section). This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

GitHub webhook parsing and HMAC-SHA256 signature verification for ATC. Translates raw GitHub webhook payloads into `atc-core` domain events. This crate is the boundary between GitHub's JSON API and the source-agnostic domain model. Internal modules are `pub(crate)` only; consumers see two public entry points and the domain event types they return.

## Sharp edges

**SHA-1 signatures are rejected, not downgraded.** ATC only accepts `sha256=` prefixed signatures. A payload signed with SHA-1 returns a hard `RejectedAlgorithm` error, not a fallback verification. If a future GitHub change moves toward a stronger algorithm it is forward-compatible; a regression toward SHA-1 would be a security downgrade and should stay rejected.

**`runner_group_name` empty-string is normalized to `None` at parse time.** GitHub's `workflow_job` payload delivers `runner_group_name` as either a missing field, a null, or an empty string depending on context. The translation layer collapses all three shapes to `None` in `RunnerInfo.group_name`. Downstream code — store, pool derivation, frontend — must not add a second normalization pass; it will already see a clean `Option` with no empty-string case.

**Serde types deliberately omit `deny_unknown_fields`.** GitHub adds fields to webhook payloads without notice. Opting into `deny_unknown_fields` on any webhook struct would cause ATC to reject otherwise-valid payloads whenever GitHub extends its schema. This is the intentional inverse of the Rust ecosystem default. Do not add `deny_unknown_fields` to webhook payload structs.

## Key References

- Architecture: `docs/architecture/backend-server.md` § GitHub API Integration
- Design plan: `docs/design-plans/2026-04-11-gh-webhooks.md`
