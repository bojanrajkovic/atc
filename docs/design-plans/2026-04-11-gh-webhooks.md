# GitHub Webhook Parsing Design

## Summary

This design implements the GitHub webhook parsing layer for the `atc-github` crate -- the boundary where raw HTTP payloads from GitHub become typed domain events that the rest of ATC can act on. When GitHub Actions workflows and jobs change state (a run is requested, a job starts waiting for a runner, a job completes), GitHub posts a JSON payload to ATC's webhook endpoint. This design covers everything needed to receive those payloads safely: verifying that the payload is genuinely from GitHub via HMAC-SHA256 signature checking, deserializing the JSON into typed Rust structs, and translating those structs into the domain event types (`RunEventEnvelope`, `JobEventEnvelope`) that `atc-core` already understands.

The implementation follows a strict boundary design: GitHub's JSON shape is an internal concern of `atc-github`. The rest of ATC only ever sees domain events -- it has no dependency on GitHub's field names or action strings. A thin translation layer bridges the two worlds, converting GitHub's stringly-typed `action` and `conclusion` fields into exhaustive domain enums and surfacing clear, structured errors when values are unrecognized. Two small additions to `atc-core` are required as prerequisites: a new `JobEvent::Waiting` variant (GitHub fires a `waiting` event when a job is blocked on an environment protection rule or required reviewer) and making the runner field on `JobEvent::InProgress` optional (GitHub occasionally fires `in_progress` before runner assignment is complete).

## Definition of Done

1. **Webhook payload types** -- serde structs in `atc-github` matching GitHub's `workflow_run` and `workflow_job` JSON shapes (only the fields ATC needs, not the full payload)
2. **Event translation** -- a function that maps raw GitHub payloads to `RunEventEnvelope`/`JobEventEnvelope` from `atc-core`, handling all action variants including `waiting`
3. **`JobEvent::Waiting` variant** -- added to `atc-core` with corresponding `StateStore` support
4. **HMAC-SHA256 verification** -- utility function in `atc-github`, optional (skipped when no secret configured, warn at startup)
5. **Deserialization tests** -- against realistic/captured GitHub webhook JSON fixtures
6. **Translation tests** -- verifying correct mapping from each action variant to domain events

**Out of scope:** OAuth device flow, REST API client (deferred to Phase 11), Axum route handlers (Phase 9), frontend.

## Acceptance Criteria

### gh-webhooks.AC1: Webhook payload deserialization
- **gh-webhooks.AC1.1 Success:** `workflow_run` JSON with `requested` action deserializes into `WorkflowRunWebhook` with all mapped fields populated
- **gh-webhooks.AC1.2 Success:** `workflow_run` JSON with null `head_commit` deserializes with `head_commit: None`
- **gh-webhooks.AC1.3 Success:** `workflow_run` JSON with null `workflow` object deserializes with `workflow: None`
- **gh-webhooks.AC1.4 Success:** `workflow_job` JSON with all four action variants (`queued`, `waiting`, `in_progress`, `completed`) deserializes correctly
- **gh-webhooks.AC1.5 Success:** `workflow_job` JSON with null `runner_id`/`runner_name` deserializes with those fields as `None`
- **gh-webhooks.AC1.6 Success:** JSON with unknown fields not in our structs deserializes without error (forward compatibility)
- **gh-webhooks.AC1.7 Failure:** Malformed JSON returns `ParseError::InvalidJson`

### gh-webhooks.AC2: Event translation
- **gh-webhooks.AC2.1 Success:** `workflow_run` `requested` maps to `RunEventEnvelope` with `RunEvent::Requested`
- **gh-webhooks.AC2.2 Success:** `workflow_run` `in_progress` maps to `RunEvent::InProgress`
- **gh-webhooks.AC2.3 Success:** `workflow_run` `completed` with `success` conclusion maps to `RunEvent::Completed { RunConclusion::Success }`
- **gh-webhooks.AC2.4 Success:** All nine `RunConclusion` variants map correctly from GitHub strings
- **gh-webhooks.AC2.5 Success:** `workflow_job` `queued` maps to `JobEvent::Queued` with labels and steps
- **gh-webhooks.AC2.6 Success:** `workflow_job` `waiting` maps to `JobEvent::Waiting` with labels and steps
- **gh-webhooks.AC2.7 Success:** `workflow_job` `in_progress` with runner info maps to `JobEvent::InProgress { runner: Some(RunnerInfo) }`
- **gh-webhooks.AC2.8 Success:** `workflow_job` `in_progress` with null runner maps to `JobEvent::InProgress { runner: None }`
- **gh-webhooks.AC2.9 Success:** `workflow_job` `completed` maps to `JobEvent::Completed` with conclusion, optional runner, labels, steps
- **gh-webhooks.AC2.10 Success:** Step data translates: `status` string to `StepStatus`, `conclusion` string to `Option<JobConclusion>`
- **gh-webhooks.AC2.11 Success:** `repository.owner.login` maps to envelope `org`, `repository.name` to `repo`
- **gh-webhooks.AC2.12 Failure:** Unknown `action` string returns `ParseError::UnknownAction` with event type and action value
- **gh-webhooks.AC2.13 Failure:** `completed` action with null `conclusion` returns `ParseError::MissingConclusion`
- **gh-webhooks.AC2.14 Failure:** Unrecognized conclusion string returns `ParseError::UnknownConclusion` with event type and value
- **gh-webhooks.AC2.15 Failure:** Unrecognized step status string returns `ParseError::UnknownStatus` with step context

### gh-webhooks.AC3: atc-core domain model updates
- **gh-webhooks.AC3.1 Success:** `JobEvent::Waiting` variant exists and carries `labels` and `steps`
- **gh-webhooks.AC3.2 Success:** `StateStore::apply_job_event` handles `Waiting` events, creating jobs in `JobStatus::Waiting`
- **gh-webhooks.AC3.3 Success:** Transition `Queued` → `Waiting` → `InProgress` succeeds
- **gh-webhooks.AC3.4 Success:** `JobEvent::InProgress` accepts `runner: None` without error
- **gh-webhooks.AC3.5 Success:** Existing tests pass with `InProgress` runner wrapped in `Some(...)`

### gh-webhooks.AC4: HMAC signature verification
- **gh-webhooks.AC4.1 Success:** Valid `sha256=<hex>` signature with correct secret passes
- **gh-webhooks.AC4.2 Failure:** Tampered body with valid signature format returns `SignatureMismatch`
- **gh-webhooks.AC4.3 Failure:** Wrong secret returns `SignatureMismatch`
- **gh-webhooks.AC4.4 Failure:** `sha1=<hex>` returns `RejectedAlgorithm`
- **gh-webhooks.AC4.5 Failure:** Unknown algorithm prefix (e.g., `sha512=`) returns `UnknownAlgorithm`
- **gh-webhooks.AC4.6 Failure:** Invalid hex after prefix returns `InvalidHex`
- **gh-webhooks.AC4.7 Failure:** Signature without `=` separator returns `InvalidFormat`
- **gh-webhooks.AC4.8 Success:** Verification uses constant-time comparison (via `hmac::Mac::verify_slice`)

### gh-webhooks.AC5: Deserialization test fixtures
- **gh-webhooks.AC5.1 Success:** Real captured JSON fixtures exist for `workflow_run` (requested, in_progress, completed)
- **gh-webhooks.AC5.2 Success:** Real captured JSON fixtures exist for `workflow_job` (queued, in_progress, completed)
- **gh-webhooks.AC5.3 Success:** Each fixture deserializes into the corresponding GitHub payload type without error

### gh-webhooks.AC6: End-to-end parse_webhook
- **gh-webhooks.AC6.1 Success:** `parse_webhook("workflow_run", body)` returns `ParseResult::Parsed(WebhookEvent::Run(...))`
- **gh-webhooks.AC6.2 Success:** `parse_webhook("workflow_job", body)` returns `ParseResult::Parsed(WebhookEvent::Job(...))`
- **gh-webhooks.AC6.3 Success:** `parse_webhook("push", body)` returns `ParseResult::Skipped { event_type: "push" }`
- **gh-webhooks.AC6.4 Success:** `parse_webhook("unknown_event", body)` returns `ParseResult::Skipped`

## Glossary

- **`atc-core`**: The domain model crate in the ATC Rust workspace. Defines the canonical types for workflow runs, jobs, and steps, the event envelopes (`RunEventEnvelope`, `JobEventEnvelope`), and the `StateStore` that applies events to in-memory state.
- **`atc-github`**: The GitHub integration crate being built by this design. Responsible for understanding GitHub's wire format and translating it into `atc-core` domain events.
- **`atc-server`**: The Axum HTTP server crate. Receives the raw HTTP webhook requests from GitHub. The actual route handler that calls into `atc-github` is deferred to Phase 9.
- **Webhook**: An HTTP POST request GitHub sends to a configured URL whenever a relevant event occurs (workflow run state changes, job state changes, etc.). ATC registers a webhook URL with GitHub to receive these notifications in real time.
- **`workflow_run` / `workflow_job`**: The two GitHub webhook event types ATC cares about. `workflow_run` events describe the top-level CI run; `workflow_job` events describe individual jobs within that run.
- **Action**: The `action` field in every GitHub webhook payload. A string like `"requested"`, `"in_progress"`, `"completed"`, or `"waiting"` that describes what state transition just occurred.
- **Conclusion**: The `conclusion` field on a completed `workflow_run` or `workflow_job` payload. A string like `"success"`, `"failure"`, `"cancelled"`, etc. Only present when `action` is `"completed"`.
- **HMAC-SHA256**: A message authentication code algorithm. GitHub signs every webhook payload with a secret the user configures; ATC verifies the signature to confirm the payload is authentic and unmodified. The signature arrives in the `X-Hub-Signature-256` HTTP header as `sha256=<hex>`.
- **Constant-time comparison**: A comparison method that takes the same amount of time regardless of where the inputs differ. Required for HMAC verification to prevent timing attacks.
- **`ParseResult`**: The return type of `parse_webhook()`. A three-way discriminant: `Parsed` (recognized event, successfully translated), `Skipped` (unrecognized event type -- not an error, just not ATC's concern), or an error via `ParseError`.
- **`pub(crate)`**: A Rust visibility modifier meaning the item is accessible only within the declaring crate. Used here to keep GitHub payload structs private to `atc-github`.
- **Copy-on-write (CoW) store mutation**: The pattern `StateStore` uses to update entities -- remove the existing entry, apply changes, re-insert -- rather than mutating in place.
- **`RunEventEnvelope` / `JobEventEnvelope`**: Wrapper types from `atc-core` that bundle a domain event with routing metadata (org, repo, workflow name, etc.) extracted from the webhook's repository fields.
- **`JobEvent::Waiting`**: A new domain event variant being added to `atc-core` as part of this design. Represents a job blocked pending approval (e.g., an environment protection rule).
- **Forward compatibility**: The property that existing code handles future inputs gracefully. Here, serde's default ignore-unknown-fields behavior means new fields GitHub adds to payloads won't break deserialization.
- **RustCrypto**: Community-maintained set of pure-Rust cryptography crates (`hmac`, `sha2`, etc.). The HMAC verification depends on these.
- **`const-hex`**: A Rust crate for hex encoding/decoding, a drop-in replacement for the `hex` crate with significantly better performance.
- **`gh webhook forward`**: A GitHub CLI extension (`cli/gh-webhook`) used during development to capture live webhook payloads from the ATC repo's CI and save them as test fixtures.

## Architecture

Opaque public API in `atc-github` with internal GitHub payload types. Two public functions: `verify_signature()` for HMAC verification and `parse_webhook()` for JSON parsing and translation to domain events. GitHub-specific serde structs are `pub(crate)` — consumers only see domain events from `atc-core`.

### Module Layout

```
backend/crates/atc-github/src/
  lib.rs                -- crate root, re-exports public API
  webhook/
    mod.rs              -- parse_webhook(), WebhookEvent, ParseResult, ParseError
    verify.rs           -- verify_signature(), VerifyError
    types.rs            -- pub(crate) GitHub payload serde structs
    translate.rs        -- pub(crate) GitHub types → domain events
```

### Public API

```rust
// Signature verification — algorithm-agnostic, driven by signature prefix
pub fn verify_signature(secret: &[u8], body: &[u8], signature: &str) -> Result<(), VerifyError>;

// Parse + translate in one step
pub fn parse_webhook(event_type: &str, body: &[u8]) -> Result<ParseResult, ParseError>;

// Three-way result separating "parsed" from "not my event"
pub enum ParseResult {
    Parsed(WebhookEvent),
    Skipped { event_type: String },
}

pub enum WebhookEvent {
    Run(RunEventEnvelope),
    Job(JobEventEnvelope),
}
```

### Error Types

```rust
pub enum VerifyError {
    InvalidFormat,        // no "algo=hex" structure
    RejectedAlgorithm,   // known but refused (e.g., sha1)
    UnknownAlgorithm,     // unrecognized — may need ATC update
    InvalidHex,           // hex decode failed
    SignatureMismatch,    // constant-time comparison failed
}

pub enum ParseError {
    InvalidJson(serde_json::Error),
    UnknownAction { event_type: String, action: String },
    MissingConclusion { event_type: String, action: String },
    UnknownConclusion { event_type: String, value: String },
    UnknownStatus { context: String, value: String },
}
```

### Internal GitHub Payload Types

`pub(crate)` serde structs matching the subset of GitHub's webhook JSON that ATC needs. Fields use native `snake_case` (matching GitHub's JSON format). Status/conclusion/action fields are `String`, parsed into domain enums during translation — unrecognized values produce clear translation errors rather than serde failures.

**Top-level payloads:**
- `WorkflowRunWebhook` — `action`, `workflow_run: WorkflowRunData`, `workflow: Option<WorkflowData>`, `repository: RepositoryData`
- `WorkflowJobWebhook` — `action`, `workflow_job: WorkflowJobData`, `repository: RepositoryData`

**Nested types (fields ATC uses):**
- `WorkflowRunData` — `id`, `status`, `conclusion`, `head_branch`, `head_sha`, `head_commit: Option<HeadCommit>`, `event`, `display_title`, `html_url`, `created_at`, `run_started_at`, `updated_at`
- `WorkflowData` — `name`, `path`
- `WorkflowJobData` — `id`, `run_id`, `name`, `status`, `conclusion`, `created_at`, `started_at`, `completed_at`, `steps: Vec<StepData>`, `labels: Vec<String>`, `runner_id`, `runner_name`, `runner_group_id`, `runner_group_name`
- `StepData` — `number`, `name`, `status`, `conclusion`, `started_at`, `completed_at`
- `RepositoryData` — `owner: OwnerData`, `name`
- `OwnerData` — `login`
- `HeadCommit` — `message`

Serde ignores unknown fields by default — forward-compatible with GitHub adding new payload fields.

### Translation Layer

Maps GitHub payload types to domain events in `translate.rs`.

**Run event mapping:**

| GitHub `action` | Domain event |
|-----------------|-------------|
| `"requested"` | `RunEvent::Requested` |
| `"in_progress"` | `RunEvent::InProgress` |
| `"completed"` | `RunEvent::Completed { conclusion }` |

Envelope fields: `repository.owner.login` → `org`, `repository.name` → `repo`, `workflow.name`/`workflow.path` → `workflow_name`/`workflow_path` (empty string fallback if `workflow` is null), `head_commit.message` → `commit_message` (nullable).

**Job event mapping:**

| GitHub `action` | Domain event |
|-----------------|-------------|
| `"queued"` | `JobEvent::Queued { labels, steps }` |
| `"waiting"` | `JobEvent::Waiting { labels, steps }` |
| `"in_progress"` | `JobEvent::InProgress { runner, labels, steps }` |
| `"completed"` | `JobEvent::Completed { conclusion, runner, labels, steps }` |

Runner info: constructed from `runner_id`, `runner_name`, `runner_group_id`, `runner_group_name`. Null runner on `in_progress` is represented as `Option<RunnerInfo> = None` (honest representation — GitHub occasionally sends null runner briefly, and only one `in_progress` event fires per job).

### HMAC Verification

Algorithm-agnostic design driven by the signature prefix (e.g., `sha256=<hex>`):

1. Split signature on first `=` to extract algorithm tag and hex digest
2. Match algorithm tag to select HMAC variant (`sha256` supported, `sha1` rejected, unknown → error)
3. Compute HMAC of body with secret using `hmac` + `sha2` crates
4. Compare via `hmac::Mac::verify_slice()` (constant-time by default)

Verification is stateless — no config, no optionality. The "optional HMAC" behavior (skip when `ATC_GITHUB__WEBHOOK_SECRET` is unset) lives in atc-server's route handler (Phase 9), not in this function.

### Changes to `atc-core`

Two additions to the domain model:

1. **`JobEvent::Waiting` variant** — `Waiting { labels: Vec<String>, steps: Vec<Step> }`. The state machine already supports `Queued → Waiting → InProgress` transitions (`job.rs:171`). `StateStore::apply_job_event` needs one new match arm: `JobEvent::Waiting { labels, steps } => (JobStatus::Waiting, None, None, labels, steps)`.

2. **`JobEvent::InProgress` runner becomes optional** — `runner: RunnerInfo` → `runner: Option<RunnerInfo>`. The store's update path already uses `runner.or(existing.runner)`, so `None` falls through to the existing value. Existing tests need `runner:` wrapped in `Some(...)`.

### Dependencies

New dependencies for `atc-github`:

| Crate | Purpose | Notes |
|-------|---------|-------|
| `atc-core` | Domain event types | Path dependency |
| `serde` + `serde_json` | Deserialization | Already in workspace |
| `chrono` | `DateTime<Utc>` in payloads | Already in workspace |
| `hmac` | HMAC computation | RustCrypto, 344M downloads |
| `sha2` | SHA-256 hash | RustCrypto, 544M downloads, v0.11.0 (Mar 2026) |
| `const-hex` | Hex decode | Drop-in replacement for `hex`, 10-50x faster |
| `tracing` | Structured logging | Already in workspace |
| `thiserror` | Error type derives | dtolnay, v2.0 stable |

## Existing Patterns

Investigation found the following patterns in `atc-core` that this design follows:

- **Serde conventions:** Structs reaching the frontend use `#[serde(rename_all = "camelCase")]`. Enums use explicit `#[serde(rename_all = "PascalCase")]`. GitHub payload types (input) need no renaming — GitHub sends `snake_case`, matching Rust field names.
- **Copy-on-write store mutations:** `StateStore` removes then re-inserts entities on update (intentional immutable CoW). Translation layer does not need to account for this — it produces envelope types, the store handles mutation.
- **Test helper factories:** `store/tests/mod.rs` provides `make_run_event()` and `make_job_event()` with sensible defaults. Similar factory functions should be created for GitHub payload types within `atc-github`'s internal tests.
- **Module decomposition at ~500 lines:** Test files split by acceptance criteria/concern area when large (`store/tests/` pattern). Property tests in top-level sibling file.
- **`#![deny(missing_docs)]` + `clippy::pedantic`:** All library crates enforce these. `atc-github` must follow suit.

No divergence from existing patterns. The opaque public API is a new pattern for this codebase (atc-core exposes all types publicly), justified by the coupling argument: atc-server should depend on domain events, not GitHub's JSON shape.

## Implementation Phases

<!-- START_PHASE_1 -->
### Phase 1: atc-core Domain Model Updates

**Goal:** Add `JobEvent::Waiting` variant and make `InProgress` runner optional — prerequisite for all translation work.

**Components:**
- `backend/crates/atc-core/src/event.rs` — add `Waiting` variant to `JobEvent`, change `InProgress` runner to `Option<RunnerInfo>`
- `backend/crates/atc-core/src/store.rs` — add `Waiting` match arm in `apply_job_event`
- `backend/crates/atc-core/src/store/tests/` — update existing tests for `Option<RunnerInfo>`, add `Waiting` ingestion and transition tests
- `backend/crates/atc-core/src/store/tests/mod.rs` — update `make_job_event` helper with `Waiting` arm

**Dependencies:** None (first phase)

**Done when:** `cargo test -p atc-core` passes with new `Waiting` event tests and updated `InProgress` tests. Covers gh-webhooks.AC3.
<!-- END_PHASE_1 -->

<!-- START_PHASE_2 -->
### Phase 2: HMAC Signature Verification

**Goal:** Algorithm-agnostic HMAC verification function with full error reporting.

**Components:**
- `backend/crates/atc-github/Cargo.toml` — add `hmac`, `sha2`, `const-hex`, `thiserror` dependencies
- `backend/crates/atc-github/src/lib.rs` — add `webhook` module, re-export public API
- `backend/crates/atc-github/src/webhook/mod.rs` — public re-exports
- `backend/crates/atc-github/src/webhook/verify.rs` — `verify_signature()` and `VerifyError`

**Dependencies:** None (independent of Phase 1)

**Done when:** Verification tests pass — valid signature, tampered body, wrong secret, `sha1=` rejected, unknown algorithm detected, malformed hex, missing prefix. Covers gh-webhooks.AC4.
<!-- END_PHASE_2 -->

<!-- START_PHASE_3 -->
### Phase 3: GitHub Payload Types and Deserialization

**Goal:** Internal serde structs that deserialize GitHub webhook JSON.

**Components:**
- `backend/crates/atc-github/Cargo.toml` — add `serde`, `serde_json`, `chrono` dependencies
- `backend/crates/atc-github/src/webhook/types.rs` — `pub(crate)` GitHub payload structs (`WorkflowRunWebhook`, `WorkflowJobWebhook`, nested types)
- `backend/crates/atc-github/tests/fixtures/` — curated JSON fixtures captured from ATC repo CI runs (via `scripts/capture-webhooks.py`)

**Dependencies:** Fixture capture (done during PR CI run for this design plan)

**Done when:** Deserialization tests pass — each action variant parses from JSON fixture, optional fields handle null, unknown fields silently ignored. Covers gh-webhooks.AC5.
<!-- END_PHASE_3 -->

<!-- START_PHASE_4 -->
### Phase 4: Translation Layer and Public API

**Goal:** Complete the parse pipeline — JSON bytes in, domain events out.

**Components:**
- `backend/crates/atc-github/Cargo.toml` — add `atc-core` path dependency, `tracing`
- `backend/crates/atc-github/src/webhook/translate.rs` — `pub(crate)` translation functions mapping GitHub types to domain events
- `backend/crates/atc-github/src/webhook/mod.rs` — `parse_webhook()`, `ParseResult`, `ParseError`, `WebhookEvent`

**Dependencies:** Phase 1 (domain model updates), Phase 3 (payload types)

**Done when:** Translation tests pass for all action variants (run: requested/in_progress/completed; job: queued/waiting/in_progress/completed). Error cases covered: unknown action, missing conclusion, unknown conclusion, unknown step status, null runner on in_progress. Covers gh-webhooks.AC1, gh-webhooks.AC2, gh-webhooks.AC6.
<!-- END_PHASE_4 -->

## Additional Considerations

**Fixture capture workflow:** The `scripts/capture-webhooks.py` script and `gh webhook forward` extension (installed via `gh extension install cli/gh-webhook`) capture real webhook payloads from the ATC repo's CI. Captured payloads land in `tmp/webhook-captures/` (gitignored), then are curated (sensitive fields scrubbed if needed) into `backend/crates/atc-github/tests/fixtures/` (committed). The PR for this design plan will itself trigger CI, providing the first batch of captures.

**Observability contract for Phase 9:** The error types carry structured fields (`event_type`, `action`, `context`, `value`) so atc-server can construct rich log lines with delivery ID context from HTTP headers. `ParseResult::Skipped` enables distinct metric tracking (`webhooks_skipped_total`) separate from errors. `VerifyError::UnknownAlgorithm` vs `RejectedAlgorithm` lets the server log at different levels (error vs warn).

**Forward compatibility:** Serde's default ignore-unknown-fields behavior means GitHub can add new payload fields without breaking deserialization. New `action` values produce `ParseError::UnknownAction` with the raw string — operators see what happened and know to upgrade ATC.

**Documents to Update:**

| Document | Change |
|----------|--------|
| `docs/architecture/backend-server.md` | Add atc-github section (webhook parsing, HMAC verification, public API contract) |
| `CLAUDE.md` | Update atc-github description from placeholder to actual functionality |
| `scripts/doc-mapping.sh` | Add mapping for atc-github source paths → architecture doc |
