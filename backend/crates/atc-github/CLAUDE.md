# CLAUDE.md -- atc-github

Last verified: 2026-05-18

> Canonical documentation lives in `docs/architecture/backend-server.md` (GitHub API Integration section). This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

The boundary crate between GitHub's HTTP APIs and ATC's source-agnostic domain model. Two concerns live here:

- **Webhook ingestion** — parsing `workflow_run` / `workflow_job` payloads and verifying HMAC-SHA256 signatures.
- **User-token OAuth client** — PKCE-flow code exchange, refresh-token rotation, and the user-token-scoped REST endpoints (`/user`, `/user/installations`, `/user/installations/{id}/repositories`).

## Modules

| Module | Role |
|--------|------|
| `webhook/mod.rs` | Public API: `parse_webhook`, `ParseError`, `ParseResult`, `WebhookEvent` |
| `webhook/verify.rs` | `verify_signature`, `VerifyError` -- HMAC-SHA256 constant-time verification |
| `webhook/types.rs` | `pub(crate)` serde structs for GitHub webhook JSON payloads |
| `webhook/translate.rs` | `pub(crate)` translation from serde types to `atc-core` domain events |
| `oauth/mod.rs` | `OAuthClient`, `TokenSet`, `PkcePair`, `generate_pkce_pair`; code-exchange + refresh against `/login/oauth/access_token` |
| `oauth/user.rs` | `OAuthClient::get_user` — `GET /user` |
| `oauth/installations.rs` | `OAuthClient::list_user_installations` and `list_installation_repositories` with Link-header pagination |
| `oauth/errors.rs` | `OAuthError` (`InvalidGrant`, `RefreshExpired`, `Unauthenticated`, `RateLimited`, `Other`) |

## Webhook contracts

Enforced by the implementation and verified by tests:

- **Two public entry points:** `verify_signature` and `parse_webhook`. All other webhook types are `pub(crate)` or private.
- **Three-way parse result:** `parse_webhook` returns `ParseResult::Parsed` for `workflow_run` and `workflow_job` events, `ParseResult::Skipped` for all other event types. Errors are reserved for actual failures (malformed JSON, unknown actions/conclusions).
- **Opaque payload types:** Consumers never see GitHub-specific serde types. The public API accepts `&[u8]` body bytes and `&str` event type, returning `atc-core` domain events.
- **Structured error context:** Every `ParseError` variant carries enough context (event type, action, value) for observability without the raw payload.
- **SHA-256 only:** `verify_signature` accepts `sha256=<hex>` signatures. SHA-1 is rejected (`RejectedAlgorithm`); unknown algorithms return `UnknownAlgorithm`.
- **Constant-time comparison:** Signature verification uses HMAC's `verify_slice` for timing-safe comparison.
- **Serializable public types:** `WebhookEvent` and `ParseResult` derive `Clone`, `Serialize` (and `Deserialize` for `WebhookEvent`), enabling downstream broadcast and REST snapshot serialization.
- **Adjacently-tagged serialization:** `WebhookEvent` uses `#[serde(tag = "type", content = "data")]` to produce discriminated unions in JSON/TypeScript (e.g., `{ type: "Run", data: { ... } }`).
- **Forward compatibility:** Serde types use default `deny_unknown_fields=false`, so new GitHub payload fields are silently ignored.
- **Empty `runner_group_name` normalization:** `make_runner_info` normalizes `runner_group_name: Some("")` to `None` — downstream never observes an empty-string group name.

## OAuth client contracts

- **Caller-owned HTTP client.** `OAuthClient::new` and `with_bases` take a `reqwest::Client` so connection pools are shared across the process. The module itself owns no global HTTP state.
- **Configurable base URLs.** `with_bases` accepts the OAuth and API bases separately, enabling mockito tests and GitHub Enterprise Server deployments. Production callers use `OAuthClient::new`, which targets `https://github.com` and `https://api.github.com`.
- **PKCE helper is free.** `generate_pkce_pair()` produces a 64-byte random verifier (base64url-no-pad, 86 chars) and the matching `S256` challenge.
- **GitHub returns OAuth errors as HTTP 200.** The code-exchange and refresh paths parse the response body first; a JSON `{"error": "...", "error_description": "..."}` shape becomes `OAuthError::InvalidGrant` (code exchange) or `OAuthError::RefreshExpired` (refresh). Only after that check does a non-2xx status become `OAuthError::Other`. Tests must mock invalid-grant responses with status 200 + an `error` body, not with 4xx.
- **Refresh rotates both tokens.** A successful `refresh_token` call returns a new `TokenSet` whose `refresh_token` is also new — callers persist the rotated value on every refresh.
- **Pagination is internal.** `list_user_installations` and `list_installation_repositories` request `per_page=100` and walk `Link: <...>; rel="next"` until absent. Link-header parsing tolerates commas inside angle-bracketed URLs. Callers receive a single `Vec` with every page concatenated.
- **REST status mapping.** `GET /user` and the paginated endpoints map 401 to `OAuthError::Unauthenticated`, 403/429 to `OAuthError::RateLimited` (with `X-RateLimit-Reset` when present), other non-2xx statuses to `OAuthError::Other`.
- **Form-encoded token endpoint.** POSTs to `/login/oauth/access_token` send `application/x-www-form-urlencoded` bodies and an explicit `Accept: application/json` header — GitHub returns form-encoded responses by default without it.

## GitHub App prerequisite (operators)

The GitHub App backing the OAuth client **must have "Expire user authorization tokens" enabled** in the app settings. With expiring tokens disabled, GitHub issues non-expiring user-to-server tokens with no refresh token; the refresh code path becomes dead code and the access-token-rotation strategy described in the auth design plan does not apply. Once enabled, access tokens last ~8h and refresh tokens last ~6 months of disuse; a successful refresh rotates both.

## TypeScript generation

`WebhookEvent` derives `#[derive(TS)]` with `#[ts(export)]`. Generated types are written to `frontend/src/lib/types/generated/` via `just types`. OAuth types are not currently exported to TypeScript (they back server-side flows only).

## Dependencies

- **Uses:** `atc-core` (domain types: `RunEvent`, `JobEvent`, envelope structs, `RunnerInfo`, `Step`, `StepStatus`, conclusions, IDs)
- **Used by:** `atc-server` (webhook route handler calls `verify_signature` then `parse_webhook`; auth routes will consume `oauth::OAuthClient` in a later slice)
- **External:** `hmac`/`sha2` for HMAC-SHA256, `const-hex` for hex decoding, `serde`/`serde_json` for JSON, `chrono` for timestamps, `thiserror` for error types, `ts-rs` for TypeScript type generation, `reqwest` (rustls + json + form) for HTTP, `base64` for PKCE encoding, `rand` for verifier randomness

## Testing

```bash
cargo nextest run -p atc-github
cargo clippy -p atc-github --all-targets -- -D warnings
```

Webhook tests use fixtures in `tests/fixtures/` curated from real CI webhook captures covering all 7 event type/action combinations. OAuth tests use `mockito::Server::new_async()` to stand up a local HTTP origin and inject its URL via `OAuthClient::with_bases`.

## Key References

- Architecture: `docs/architecture/backend-server.md` § GitHub API Integration
- Webhook design plan: `docs/design-plans/2026-04-11-gh-webhooks.md`
- Auth design plan: `docs/design-plans/2026-05-16-github-auth-and-repo-scoping.md`
