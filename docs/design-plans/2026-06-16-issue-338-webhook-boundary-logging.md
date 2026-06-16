# Webhook Boundary-Event Logging (issue #338)

Last verified: 2026-06-16

## Context

ATC ingests GitHub webhooks at `POST /v1/webhooks/github`
(`backend/crates/atc-server/src/routes.rs:184-343`). The handler emits exactly one
INFO line for the happy path — `tracing::info!(event_type, seq, "event accepted")`
at `routes.rs:288` — so routine `workflow_run` / `workflow_job` traffic is visible
at the default `info` filter. A few boundary outcomes are not visible at that filter,
which made the 2026-05-21 homelab smoke-test harder than necessary (issue #338):

- **`ping` is logged only at DEBUG.** GitHub fires a `ping` on webhook creation.
  Today `parse_webhook` routes any unrecognized event type (including `"ping"`) to
  `ParseResult::Skipped` (`backend/crates/atc-github/src/webhook/mod.rs:147-152`),
  and the handler logs it with `tracing::debug!(event_type, "event skipped")`
  (`routes.rs:327`). At the default `info` filter there is no log signal that the
  webhook is wired up — an operator must inspect `gh api .../hooks/.../deliveries`.
- **Skipped events and parse failures lack the correlation fields the issue asks
  for.** The parse-failure ERROR line (`routes.rs:264`) already carries `event_type`
  but not `delivery_id`. The `event accepted` line carries `seq` but not `run_id`.
  `delivery_id` is recorded on the `webhook.handler` span at `routes.rs:216` but is
  not on any of the boundary log *lines*, so it is absent in pretty (non-span-list)
  log output.

Note two of the issue's premises are already partly satisfied and the plan should
not "re-add" them: parse *failures* already log at ERROR, and `ping` already logs
(just at DEBUG). The real work is **raising the level of the skip/ping path so it is
visible by default** and **enriching the boundary lines with `delivery_id` / `run_id`**.

The relevant pieces are small and well-bounded:
- `atc-github` `ParseResult` enum + `parse_webhook` (`webhook/mod.rs:59-154`).
- `atc-server` `webhook_handler` match arms (`routes.rs:261-333`).
- The integration test harness already has in-memory OTel span capture
  (`ensure_span_exporter_installed`, `read_finished_spans`) and behavioral
  `oneshot` webhook tests (`tests/integration/webhook_ingestion_tests.rs`,
  `tracing_webhook_spans_test.rs`). There is no log-*event* capture helper yet
  (`tracing-test` is not a dependency), so asserting log levels/fields needs a
  small in-memory `tracing` layer modeled on the existing span-exporter helper.

## Definition of Done

**Primary deliverables**

1. At the default `info` filter, every webhook outcome produces a log line:
   - `ping` → INFO `"ping received"` with `event_type` + `delivery_id`.
   - any other unhandled event type (`push`, `pull_request`, …) → INFO
     `"event skipped"` with `event_type` + `delivery_id`.
   - parse failure → ERROR `"webhook parse error"` with `error` + `event_type` +
     `delivery_id` (adds `delivery_id` to the existing line).
   - state transition committed → INFO `"event accepted"` with `event_type` + `seq`
     + `run_id` (and `job_id` for job events).
2. `ping` is a first-class outcome at the parse boundary: a `ParseResult::Ping`
   variant returned by `parse_webhook` for `event_type == "ping"`.
3. Tests cover the new variant, the handler's response/level/fields for each arm,
   and the unchanged HTTP status codes.
4. `docs/architecture/backend-server.md` and `docs/architecture/metrics.md` describe
   the webhook boundary-log inventory and its level policy.

**Success criteria** — `just test` green; a `ping` POST at default filter emits an
INFO `ping received` line carrying `delivery_id`; an unknown-type POST emits INFO
`event skipped`; an accepted event's line carries `run_id`; a parse failure's line
carries `delivery_id`. HTTP status codes are unchanged for every arm.

**Key exclusions** — no new metrics, no OTLP log exporter (logs stay on the stderr
subscriber per metrics.md:36), no change to HTTP status codes, no parsing of the
ping payload body (`zen` / `hook_id` are not extracted).

## Locked Decisions

These were settled with the issue author during planning (2026-06-16) and are not
open for re-evaluation:

1. **All skipped event types log at INFO** (not DEBUG-with-logFilter, not ping-only).
   Every unhandled event type becomes visible at the default `info` filter. Rationale:
   the issue's core complaint is default-filter invisibility; the homelab webhook is
   subscribed to a narrow event set, so INFO-level skip volume is acceptable.
2. **`ping` is modeled as a dedicated `ParseResult::Ping` variant** in `atc-github`,
   not a server-side `event_type == "ping"` string check. Rationale: ping is
   recognized at the GitHub boundary where event types are already matched, the
   server's `match` stays exhaustive/intentional, and ping behavior is unit-testable
   in `atc-github`.
3. **Logs stay out of the OTel pipeline** — `tracing::{info,warn,error}!` only, per
   `docs/architecture/metrics.md:36`. No `LoggerProvider`.

## Architecture

### Three changes, two crates

**A. `atc-github` — add `ParseResult::Ping` (`webhook/mod.rs`).**

```rust
pub enum ParseResult {
    Parsed(Box<WebhookEvent>),
    /// A GitHub `ping` event — webhook connectivity check. Carries no payload;
    /// the connectivity signal is the delivery itself.
    Ping,
    Skipped { event_type: String },
}
```

Add a `"ping"` arm to `parse_webhook` *before* the catch-all:

```rust
"ping" => {
    tracing::debug!("ping webhook received");
    Ok(ParseResult::Ping)
}
_ => { /* unchanged: Skipped */ }
```

`ParseResult` derives `serde::Serialize` (`webhook/mod.rs:60`) but is **not**
`ts-rs`-exported (only `WebhookEvent` carries `#[ts(export)]`), so the new variant
adds no frontend wire surface and needs no `just types` regen. The body is not
deserialized for ping — the variant is a unit marker.

*Rejected:* server-side `event_type == "ping"` check (Locked Decision 2). It would
scatter a magic string into the server and keep ping untestable at the parse layer.

**B. `atc-server` — boundary log levels + fields (`routes.rs`).**

1. Capture `delivery_id` once into a local for reuse on log lines (today it is only
   recorded onto the span at `routes.rs:212-217`):

   ```rust
   let delivery_id = headers
       .get("x-github-delivery")
       .and_then(|v| v.to_str().ok());
   if let Some(d) = delivery_id {
       Span::current().record("webhook.delivery_id", d);
   }
   ```
   Log lines reference it with the `?` sigil (`delivery_id = ?delivery_id`) so the
   `Option` renders cleanly when absent.

2. Parse-failure arm (`routes.rs:264`) — add `delivery_id`:
   `tracing::error!(error = %e, event_type, delivery_id = ?delivery_id, "webhook parse error");`

3. Accepted arm (`routes.rs:288`) — add `run_id` (and `job_id` for jobs). `event` is
   still in scope after `apply_*`, so match it by reference:
   ```rust
   Ok(seq) => {
       match &event {
           WebhookEvent::Run(env) => tracing::info!(
               event_type, seq, run_id = env.run_id.0,
               delivery_id = ?delivery_id, "event accepted"),
           WebhookEvent::Job(env) => tracing::info!(
               event_type, seq, run_id = env.run_id.0, job_id = env.job_id.0,
               delivery_id = ?delivery_id, "event accepted"),
       }
       (StatusCode::OK, Json(json!({"status": "accepted", "seq": seq})))
   }
   ```

4. New `ParseResult::Ping` arm — INFO, returns 200:
   ```rust
   ParseResult::Ping => {
       tracing::info!(event_type, delivery_id = ?delivery_id, "ping received");
       (StatusCode::OK, Json(json!({"status": "ok"})))
   }
   ```
   `{"status": "ok"}` is a new body for a new variant; no prior contract asserts a
   ping body, and existing tests only assert `"skipped"` for non-ping types.

5. Skipped arm (`routes.rs:327`) — DEBUG → INFO, add `delivery_id`:
   `tracing::info!(event_type, delivery_id = ?delivery_id, "event skipped");`

The compiler enforces exhaustive handling of the new variant, so the Ping arm cannot
be silently dropped.

### Test strategy

- **`atc-github` unit test** (the primary TDD anchor): `parse_webhook("ping", b"{}")`
  returns `ParseResult::Ping`; `parse_webhook("push", b"{}")` still returns `Skipped`.
  Lives beside the existing skip tests at `webhook/mod.rs:156+`.
- **`atc-server` behavioral test**: ping POST → 200 with `{"status":"ok"}`; unknown
  type still → `{"status":"skipped"}`. Extends `webhook_ingestion_tests.rs`.
- **Log-content test**: add an in-memory log-capture helper to
  `tests/integration/common/mod.rs` modeled on `ensure_span_exporter_installed`
  (`common/mod.rs:168`): a `tracing_subscriber::Layer` that pushes
  `{ level, message, fields }` into a process-global `Mutex<Vec<_>>`, installed once
  and reset per test, with `#[serial]` (OTel/global-subscriber rule, atc-server
  CLAUDE.md). Tests assert: `ping received` at INFO with `delivery_id`; `event
  skipped` at INFO; `event accepted` carries `run_id`; `webhook parse error` carries
  `delivery_id`. `tracing-test` is intentionally *not* added — the helper mirrors the
  house in-memory-exporter pattern and keeps the dependency set unchanged.

### Not ADR-worthy

This is a log-level/field-enrichment change governed by an existing convention
(metrics.md § "Logs are not in the OTel pipeline"). The level policy is recorded in
`metrics.md` (the observability doc), not a standalone ADR. No architectural boundary
moves.

## Implementation Steps

TDD-ordered. Step 1 writes failing tests; step 2 makes them pass.

1. **Write failing tests.** Add the `parse_webhook("ping", …) → Ping` unit test in
   `atc-github`; add the ping/skip behavioral tests and the log-capture helper +
   log-content assertions in `atc-server`. Confirm they fail to compile / fail
   (no `Ping` variant; ping currently returns `"skipped"`; skip/ping at DEBUG).
2. **Make them pass.** Add `ParseResult::Ping` + the `"ping"` arm in `atc-github`;
   update the `webhook_handler` arms in `atc-server` (capture `delivery_id`; raise
   skip to INFO; add the Ping arm; enrich accepted + parse-failure lines).
3. **Update docs.** Add the webhook boundary-log inventory + level policy to
   `backend-server.md` and `metrics.md` (see Documents to Update).
4. **Verify end to end.** `cd /home/user/atc/backend && cargo nextest run` for both
   crates; `just lint`; spot-check a local `ping` POST shows the INFO line at the
   default filter.

## Acceptance Criteria

- **AC1 — Ping variant.** `parse_webhook("ping", b"{}")` returns `ParseResult::Ping`.
  *Failure case:* it returns `Skipped { event_type: "ping" }` (regression to old
  behavior) → test fails.
- **AC2 — Ping log + response.** A `ping` POST at the default `info` filter emits an
  INFO `ping received` line carrying `event_type` and `delivery_id`, and returns
  `200` with `{"status":"ok"}`. *Failure case:* no INFO line at default filter, or
  `delivery_id` absent, or status ≠ 200.
- **AC3 — Skipped at INFO.** An unknown event type (`push`) emits an INFO
  `event skipped` line with `event_type` + `delivery_id` and returns `200` with
  `{"status":"skipped"}`. *Failure case:* line emitted only at DEBUG, or body changed.
- **AC4 — Accepted enrichment.** An accepted `workflow_run` emits `event accepted` at
  INFO with `seq` **and** `run_id`; an accepted `workflow_job` additionally carries
  `job_id`. *Failure case:* `run_id` missing from the line.
- **AC5 — Parse-failure enrichment.** A malformed body emits `webhook parse error` at
  ERROR with `event_type` **and** `delivery_id`, status `422` unchanged. *Failure
  case:* `delivery_id` missing, or status changed.
- **AC6 — No status-code drift.** Status codes are unchanged across all arms (ping
  200, skip 200, accept 200, reject 200, parse-fail 422, missing-header 400). The
  existing `webhook_ingestion_tests.rs` / `webhook_hmac_tests.rs` suites pass
  unmodified except where they assert the new ping/skip behavior.
- **AC7 — Docs updated.** `backend-server.md` and `metrics.md` list the four boundary
  log events with their levels and fields; doc-staleness gate passes.

## Documents to Update

- **`docs/architecture/backend-server.md`** — gate-required (both `atc-github/src/*`
  and `atc-server/src/routes.rs` map here, `scripts/doc-mapping.yaml:56-60,123-127`).
  Add a "Webhook boundary logging" note to the ingestion section: the four log events
  (`ping received`, `event skipped`, `event accepted`, `webhook parse error`), their
  levels (INFO/INFO/INFO/ERROR), fields, and the `ParseResult::Ping` outcome.
- **`docs/architecture/metrics.md`** — extend the logging-conventions area
  (around `metrics.md:36`) with the boundary-log inventory and the "all skipped event
  types log at INFO" level policy (Locked Decision 1). This is the observability doc
  and the canonical home for the log-level policy.
- **`scripts/doc-mapping.yaml`** — no change. Existing mappings already couple both
  source paths to `backend-server.md`; the `metrics.md` update is voluntary (correct
  home for the policy) and does not need a new mapping entry.
- **`backend/crates/atc-github/CLAUDE.md` / `atc-server/CLAUDE.md`** — no Tier-2
  sharp edge expected; add one only if implementation surfaces a non-obvious foot-gun
  (e.g., the `delivery_id` capture-before-span-record ordering).

## Out of Scope

- Parsing the ping payload (`zen`, `hook_id`, `hook.config`) into structured fields —
  the delivery itself is the connectivity signal (issue #338 asks only for "ping
  accepted"). A future issue can enrich if operators want hook identity.
- Webhook ingestion metrics (counters per outcome) — issue #338 is logging-only;
  metrics would be a separate observability increment.
- An OTLP log exporter / `LoggerProvider` — explicitly out per metrics.md:36.

## Glossary

- **Boundary event** — a webhook outcome at the ingestion edge that is neither a
  normal accepted state transition nor an HTTP-protocol error: ping, skipped
  (unhandled type), and parse failure.
- **`ParseResult`** — the three-way (post-change: four-way) result of
  `atc_github::parse_webhook`: `Parsed`, `Ping`, `Skipped`. Not a wire/`ts-rs` type.
- **`delivery_id`** — the `X-GitHub-Delivery` header value; GitHub's per-delivery
  correlation id, used to cross-reference `gh api .../hooks/.../deliveries`.
