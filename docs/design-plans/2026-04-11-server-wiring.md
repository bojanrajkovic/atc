# Server Wiring Design

## Summary

ATC's backend is structured as three cooperating Rust crates: `atc-core` (the domain model and state store), `atc-github` (webhook parsing and signature verification), and `atc-server` (the Axum HTTP server). Until this phase, these crates have been developed independently — `atc-core` manages state, `atc-github` translates raw GitHub payloads into domain events, but nothing connects them to a live server that clients can talk to.

Phase 9 wires all three together. It adds three HTTP endpoints to `atc-server`: a webhook ingestion route that receives GitHub events, verifies their authenticity, and applies them to the shared state store; a WebSocket endpoint that pushes those same events to all connected dashboard clients in real time; and a REST snapshot endpoint that lets clients fetch the full current state on startup. The design keeps mutation and delivery separate — the webhook route writes to the store and publishes to a broadcast channel, while the WebSocket and REST endpoints only read. This separation is intentional: it anticipates a future where state moves to an external database, at which point each path can evolve independently without touching the others.

## Definition of Done

Phase 9 wires together the `atc-github` webhook parsing layer and the `atc-core` state store through the existing `atc-server` Axum application. When complete:

1. **Webhook ingestion** — `POST /v1/webhooks/github` accepts GitHub webhook payloads, verifies HMAC-SHA256 signatures (when a secret is configured), parses payloads via `atc_github::parse_webhook`, and feeds the resulting domain events into a shared `StateStore`. Returns appropriate HTTP status codes for processed, skipped, and error cases.

2. **WebSocket endpoint** — `/v1/ws` accepts WebSocket upgrades and pushes state updates to connected clients when webhook events mutate the store. Initial state delivery strategy (snapshot-on-connect vs separate REST endpoint) to be resolved during design.

3. **Server lifecycle wiring** — `StateStore` is created in `main.rs` with `SystemClock`, shared as `Arc<StateStore>` via Axum state, with background eviction task started.

4. **Config** — `ATC_GITHUB__WEBHOOK_SECRET` (optional) added to the existing figment-based `Config`.

5. **Integration tests** — Webhook ingestion (valid/invalid/missing signature, known/unknown event types), WebSocket connection and message receipt, end-to-end webhook-to-store-to-WebSocket flow.

**Out of scope:** Per-user WebSocket filtering (Phase 11), frontend components (Phase 10), OAuth (Phase 11), build.rs frontend trigger (handled by `just build` and CI pipeline).

## Acceptance Criteria

### server-wiring.AC1: Webhook HMAC Verification
- **server-wiring.AC1.1 Success:** Valid `sha256=<hex>` signature with matching secret returns 200
- **server-wiring.AC1.2 Success:** No secret configured + no signature header → verification skipped, returns 200
- **server-wiring.AC1.3 Failure:** Invalid signature returns 401
- **server-wiring.AC1.4 Failure:** Missing `X-Hub-Signature-256` when secret is configured returns 401
- **server-wiring.AC1.5 Failure:** SHA-1 signature (`sha1=...`) rejected with 401

### server-wiring.AC2: Webhook Parsing and Ingestion
- **server-wiring.AC2.1 Success:** `workflow_run` event parsed and applied to StateStore, returns `{"status": "processed"}`
- **server-wiring.AC2.2 Success:** `workflow_job` event parsed and applied to StateStore, returns `{"status": "processed"}`
- **server-wiring.AC2.3 Success:** Unknown event type (e.g., `push`) returns `{"status": "skipped"}`
- **server-wiring.AC2.4 Failure:** Missing `X-GitHub-Event` header returns 400
- **server-wiring.AC2.5 Failure:** Malformed JSON body returns 422
- **server-wiring.AC2.6 Edge:** Backward state transition (e.g., `completed` run receiving `in_progress`) returns 200 and logs warning (not 500)
- **server-wiring.AC2.7 Success:** Processed event is broadcast as a `SeqEvent` with a monotonically increasing `seq` value
- **server-wiring.AC2.8 Success:** Consecutive webhook ingestions produce strictly increasing `seq` values (no gaps, no duplicates)

### server-wiring.AC3: WebSocket Event Stream
- **server-wiring.AC3.1 Success:** `GET /v1/ws` upgrades to WebSocket connection
- **server-wiring.AC3.2 Success:** Connected client receives `SeqEvent` (domain event + seq) after webhook ingestion
- **server-wiring.AC3.3 Success:** Multiple connected clients each receive the same `SeqEvent`
- **server-wiring.AC3.4 Edge:** Client disconnect does not crash the server or affect other clients
- **server-wiring.AC3.5 Edge:** Lagging client receives warning log, continues receiving (not disconnected)

### server-wiring.AC4: REST State Snapshot
- **server-wiring.AC4.1 Success:** `GET /v1/state` returns `seq: 0` and empty collections when no events have been ingested
- **server-wiring.AC4.2 Success:** `GET /v1/state` returns runs, jobs, pool stats, and current `seq` after webhook ingestion
- **server-wiring.AC4.3 Success:** Response includes `pool_stats` computed from live job state
- **server-wiring.AC4.4 Success:** The `seq` in the REST response is consistent with the store state — all events with `seq <= N` are reflected in the snapshot

### server-wiring.AC5: End-to-End Flow
- **server-wiring.AC5.1 Success:** POST webhook → GET /v1/state reflects the ingested event and returns matching `seq`
- **server-wiring.AC5.2 Success:** POST webhook → WS client receives `SeqEvent` with matching domain event
- **server-wiring.AC5.3 Success:** Multi-event sequence (run requested → job queued → job in_progress) produces state readable via REST and events observable via WS, with `seq` values strictly increasing across the sequence

### server-wiring.AC6: Config and Lifecycle
- **server-wiring.AC6.1 Success:** `ATC_GITHUB__WEBHOOK_SECRET` is loaded from environment into config
- **server-wiring.AC6.2 Success:** Omitting `ATC_GITHUB__WEBHOOK_SECRET` results in `None` (HMAC verification skipped)
- **server-wiring.AC6.3 Success:** StateStore eviction task is started and runs in background

## Glossary

- **Axum**: A Rust web framework built on `tokio` and `hyper`. Used in `atc-server` to define HTTP routes and extract typed request data via extractors.
- **broadcast channel (`tokio::sync::broadcast`)**: A multi-producer, multi-consumer channel where every active receiver gets every message. Bounded buffer means slow consumers miss messages rather than blocking the sender.
- **domain event**: A typed Rust struct representing something that happened — e.g., a workflow run transitioning to `in_progress`. `atc-core` defines these; `atc-github` produces them from raw GitHub payloads.
- **figment**: A Rust configuration library that merges values from multiple sources (defaults, environment variables) into a typed config struct. Uses `ATC_` prefix and `__` as the struct-nesting separator.
- **HMAC-SHA256**: A message authentication code algorithm. GitHub signs webhook payloads with a shared secret and sends the result in `X-Hub-Signature-256`. The server recomputes the signature to verify payload integrity.
- **`oneshot()` (tower)**: A test utility that sends a single HTTP request directly through a router without binding to a network port.
- **`RecvError::Lagged`**: Error from a `tokio::sync::broadcast` receiver when the channel buffer has overwritten unread messages. The handler logs a warning; the client recovers via `GET /v1/state`.
- **`seq`**: An ATC-owned monotonic counter (`AtomicU64`) incremented on each successfully ingested event. Provides an ordering cursor for snapshot/stream reconciliation. Resets on server restart (consistent with in-memory store). Maps to `BIGSERIAL` when state externalizes to a database.
- **`SeqEvent`**: A broadcast message wrapping a domain event (`WebhookEvent`) with its assigned `seq`. Carried over the WebSocket as JSON, e.g., `{ "seq": 42, "event": { ... } }`.
- **StateStore**: The in-memory store in `atc-core` holding all live `WorkflowRun` and `Job` state. Mutations through typed event methods; reads return owned snapshots.
- **`StoreError`**: Error from `StateStore` when an event represents an illegal state transition. The webhook handler treats this as a warning, not a 500, because retrying would fail identically.
- **TTL eviction**: Automatic removal of completed state entries after a configured time-to-live. A background task periodically purges stale runs and jobs.
- **WebSocket**: A persistent TCP connection established via HTTP upgrade. Used here as a one-way push channel — server sends domain events; clients send nothing in Phase 9.
- **`#[tracing::instrument]`**: A Rust macro that creates a structured tracing span around a function. `skip(body)` excludes raw payloads from logs for security.

## Architecture

Phase 9 introduces three new HTTP endpoints and wires together the existing `atc-core` state store with the `atc-github` webhook parser through the Axum server. The design separates two concerns: **state mutation** (webhook → store) and **state delivery** (store → clients via REST and WebSocket).

### Application State

A shared `AppState` struct is passed to all handlers via Axum's `State` extractor:

```rust
struct AppState {
    store: Arc<StateStore>,
    webhook_tx: broadcast::Sender<SeqEvent>,
    webhook_secret: Option<String>,
    seq: AtomicU64,
}
```

`StateStore` is created in `main.rs` with `SystemClock` and a configurable TTL. The broadcast channel is a `tokio::sync::broadcast` with a fixed capacity buffer. Both are shared by reference — handlers receive `State<Arc<AppState>>`.

`seq` is an ATC-owned monotonic counter incremented on every successfully ingested event. Each broadcast message carries the assigned `seq`, and the REST snapshot response includes the current `seq` at read time. Clients use `seq` — not entity timestamps — to reconcile the snapshot/stream handoff (see [Snapshot/Stream Reconciliation](#snapshotstream-reconciliation)). The counter resets on server restart, which is consistent: the in-memory store also starts empty on restart.

`PrometheusMetricLayer` is **not** part of `AppState`. It is applied to the router via `.layer()` during router construction in `main.rs`. The existing `OnceLock`-based initialization pattern for metrics is unchanged.

### Webhook Ingestion

`POST /v1/webhooks/github` is the write path. The handler:

1. Extracts `X-GitHub-Event` and `X-Hub-Signature-256` headers
2. Verifies HMAC-SHA256 via `atc_github::verify_signature` (skipped when `webhook_secret` is `None`)
3. Parses via `atc_github::parse_webhook`
4. Applies domain events to the `StateStore`
5. Assigns `seq` via `state.seq.fetch_add(1, Relaxed)` — the assigned value becomes the event's ordering cursor
6. Broadcasts a `SeqEvent { seq, event }` through the `tokio::sync::broadcast` channel

The broadcast channel is decoupled from the store — it carries self-contained domain events wrapped with an ATC-assigned `seq`. This separation is deliberate: when state is externalized to a database, the broadcast swaps to `pg NOTIFY` or Redis pub/sub while the WebSocket handler code remains unchanged. The in-memory `AtomicU64` maps directly to a `BIGSERIAL` primary key in a future event/outbox table.

`StoreError` (invalid state transition) returns HTTP 200, not 500. A backward transition means a later event was already processed — retrying would fail identically. GitHub retries on 4xx/5xx, so returning an error would create unnecessary retry traffic.

### WebSocket Event Stream

`GET /v1/ws` is the real-time push path. On upgrade, each connection subscribes to the broadcast channel and forwards `SeqEvent`s as `Message::Text(json)`. Each message carries the event's `seq`, giving the client an ordering cursor. The WebSocket is a pure event pipe — no store reads occur on this path.

Slow consumers receive `RecvError::Lagged(n)` from the broadcast channel. The handler logs a warning and continues. Clients can recover by re-fetching `GET /v1/state` and using the returned `seq` to discard stale buffered events. No disconnection on lag — too aggressive for a monitoring dashboard.

No client-to-server messages in Phase 9. Phase 11 adds subscription filtering for per-user access control.

### REST State Snapshot

`GET /v1/state` is the backfill path. Returns a JSON snapshot of all current runs, jobs, and pool stats, plus the current `seq` value at the time of the read. Clients use this `seq` to reconcile with the WebSocket stream (see [Snapshot/Stream Reconciliation](#snapshotstream-reconciliation)).

This separation (REST for snapshots, WS for events) is future-proof: when state externalizes to a database, the REST endpoint becomes a DB query, and the WS channel stays a thin event pipe. Both paths remain independent.

In Phase 9, with no auth, this returns all state. Phase 11 scopes it to the authenticated user's repos via the existing `query_by_repos` method.

### Snapshot/Stream Reconciliation

Clients connect to the WebSocket and fetch the REST snapshot in parallel. The handoff uses `seq`, not entity timestamps:

1. Client connects to `/v1/ws`, starts buffering `SeqEvent`s
2. Client fetches `GET /v1/state`, receives `{ runs, jobs, pool_stats, seq: N }`
3. Client discards any buffered WS events with `seq <= N` (already reflected in snapshot)
4. Client applies remaining buffered events and continues with the live stream

This protocol is lossless as long as the WS connection stays subscribed during the REST fetch. If the broadcast channel lags during that window, the client re-fetches (back to step 2).

**Consistency note:** The REST snapshot reads the store under `RwLock` and captures the current `seq` atomically relative to the store state. Because the webhook handler applies the event to the store *before* incrementing `seq` and broadcasting, a snapshot at `seq: N` is guaranteed to reflect all events up to and including `N`.

**Future externalization:** The in-memory `AtomicU64` maps to a `BIGSERIAL` / identity column on an event/outbox table. The REST snapshot returns the max committed `seq`; the WS stream publishes committed events with their `seq`. Same client protocol, durable ordering. See GitHub issue #7.

### Data Flow

```
GitHub ──webhook──▶ POST /v1/webhooks/github
                         │
                         ├─▶ verify_signature (HMAC)
                         ├─▶ parse_webhook (JSON → domain event)
                         ├─▶ store.apply_{run,job}_event()
                         ├─▶ seq = state.seq.fetch_add(1)
                         └─▶ broadcast.send(SeqEvent { seq, event })
                                   │
                    ┌──────────────┼──────────────┐
                    ▼              ▼              ▼
                WS client 1   WS client 2   WS client N
                (subscribe)   (subscribe)   (subscribe)

Client startup (parallel):
  1. connect('/v1/ws')      → buffer SeqEvents
  2. fetch('/v1/state')     → snapshot + seq: N
  3. discard events ≤ N     → apply remaining
```

### Config

One new nested field in the existing figment-based `Config`:

```rust
struct GitHubConfig {
    webhook_secret: Option<String>,
}
```

Configured via `ATC_GITHUB__WEBHOOK_SECRET`. The `__` separator maps to struct nesting, consistent with the existing `ATC_*` prefix and the separator pattern already documented in `config.rs`.

### REST Response Shape

`GET /v1/state` returns:

```rust
struct StateSnapshot {
    seq: u64,
    runs: Vec<WorkflowRun>,
    jobs: Vec<Job>,
    pool_stats: Vec<RunnerPoolStats>,
}
```

`WorkflowRun`, `Job`, and `RunnerPoolStats` are already `Serialize`-derived in `atc-core`. The JSON serialization uses `serde`'s default field naming (snake_case), matching the Rust struct field names. No envelope or pagination — the in-memory store holds at most hundreds of active entities.

`SeqEvent` on the WebSocket:

```rust
struct SeqEvent {
    seq: u64,
    event: WebhookEvent,
}
```

Where `WebhookEvent` is the existing `atc_github::WebhookEvent` enum (`Run(RunEventEnvelope)` / `Job(JobEventEnvelope)`), already `Serialize`-derived on all inner types.

### Response Codes

| Case | Status | Body |
|------|--------|------|
| Processed run/job event | 200 | `{"status": "processed"}` |
| Skipped event type | 200 | `{"status": "skipped"}` |
| Missing `X-GitHub-Event` | 400 | `{"error": "missing X-GitHub-Event header"}` |
| Missing/invalid HMAC signature | 401 | `{"error": "..."}` |
| JSON parse or translation error | 422 | `{"error": "..."}` |
| Invalid state transition (backward) | 200 | `{"status": "processed"}` (logged as warning) |

### Observability

Every decision point in the webhook and WebSocket paths emits structured tracing:

| Event | Level | Fields |
|-------|-------|--------|
| Webhook received | `debug` | event_type |
| Event processed | `info` | event_type, run_id or job_id, org, repo |
| Event skipped | `debug` | event_type |
| HMAC failure | `warn` | (no payload details for security) |
| Parse error | `error` | ParseError context (event_type, action, value) |
| Store transition error | `warn` | transition details |
| WS client connected | `info` | peer_addr |
| WS client disconnected | `info` | peer_addr |
| WS client lagging | `warn` | missed_count |
| WS event forwarded | `debug` | event_type |

Webhook handler uses `#[tracing::instrument]` with `skip(body)` (security — no raw payloads in logs), consistent with the instrumentation in `atc-github`.

## Existing Patterns

### Config

The existing `Config` in `config.rs` uses figment with `ATC_` prefix and `__` as hierarchy separator. The `load()` method merges defaults with env vars. The new `GitHubConfig` nested struct follows this exact pattern — `ATC_GITHUB__WEBHOOK_SECRET` maps naturally.

### Routes

`api_routes()` in `routes.rs` currently takes a `PrometheusMetricLayer` and returns a `Router`. This changes to `api_routes(prometheus_layer) -> Router<Arc<AppState>>` — the function still receives the prometheus layer as a parameter (it's middleware, not state) and returns a router that expects `AppState` to be provided via `.with_state()` in `main.rs`. The existing health endpoints (`/healthz`, `/readyz`) and removed-endpoint-404 handler remain unchanged.

### Test Infrastructure

Integration tests in `tests/` use two patterns:
- **Route-level:** `tower::ServiceExt::oneshot()` for single-request tests without spawning a server
- **Full-stack:** Ephemeral `TcpListener::bind("127.0.0.1:0")` with `tokio::spawn` for tests requiring real network I/O

Both patterns use `OnceLock<PrometheusMetricLayer>` with `#[serial_test::serial]` to avoid global metrics recorder conflicts. New tests follow these established patterns.

### StateStore API

The store's event-driven API (`apply_run_event`, `apply_job_event`) accepts envelope structs and returns `Result<(), StoreError>`. The `query_by_repos` method returns owned snapshots. A new `query_all` convenience method is needed for the unfiltered REST endpoint.

## Implementation Phases

<!-- START_PHASE_1 -->
### Phase 1: Config and AppState Wiring

**Goal:** Extend config with webhook secret, create `AppState` struct, wire `StateStore`, broadcast channel, and seq counter into `main.rs`. Clarify lifecycle ownership of the eviction task.

**Components:**
- `backend/crates/atc-server/src/config.rs` — add `GitHubConfig` nested struct with `webhook_secret: Option<String>`
- `backend/crates/atc-server/src/main.rs` — create `Arc<StateStore>`, broadcast channel, `AtomicU64` seq counter, `AppState`; start eviction task; pass state to router via `.with_state()`; `PrometheusMetricLayer` stays as a `.layer()` call, not part of `AppState`
- `backend/crates/atc-server/src/routes.rs` — change `api_routes` to return `Router<Arc<AppState>>` (still receives prometheus layer as parameter for `.layer()`)
- `backend/crates/atc-server/src/lib.rs` — export new modules
- `backend/crates/atc-server/tests/config_tests.rs` — test new config field
- `backend/crates/atc-server/tests/routes_tests.rs` — update to use new `api_routes` signature

**Eviction task lifecycle:** `start_eviction_task` returns a `JoinHandle`. `main.rs` holds this handle and aborts it when the `CancellationToken` fires (inside the existing `tokio::select!` shutdown path). The eviction task does not need its own cancellation awareness — aborting the handle is sufficient since the task only does periodic store sweeps with no external I/O to clean up.

**Dependencies:** None (first phase)

**Done when:** `cargo check -p atc-server` succeeds, existing tests updated and passing, `ATC_GITHUB__WEBHOOK_SECRET` is loadable from environment
<!-- END_PHASE_1 -->

<!-- START_PHASE_2 -->
### Phase 2: Webhook Route Handler

**Goal:** Accept GitHub webhooks, verify signatures, parse payloads, ingest into StateStore, assign seq, broadcast events.

**Components:**
- `backend/crates/atc-server/src/routes.rs` — add `POST /v1/webhooks/github` handler with header extraction, HMAC verification, parsing, store ingestion, seq assignment via `AtomicU64::fetch_add`, and broadcast of `SeqEvent`
- `backend/crates/atc-server/tests/webhook_tests.rs` — Tier 1 route-level tests using `oneshot()`

**Dependencies:** Phase 1 (AppState, config)

**Done when:** Tests verify: valid webhook → 200 processed, invalid signature → 401, missing event header → 400, parse error → 422, skipped event → 200, no-secret-configured → 200 without signature, broadcast message carries monotonically increasing seq. Covers `server-wiring.AC1.*` and `server-wiring.AC2.*`.
<!-- END_PHASE_2 -->

<!-- START_PHASE_3 -->
### Phase 3: WebSocket Endpoint

**Goal:** Accept WebSocket upgrades, subscribe to broadcast channel, forward domain events to connected clients.

**Components:**
- `backend/crates/atc-server/src/ws.rs` — WebSocket upgrade handler, per-connection task with broadcast subscription, message forwarding, disconnect cleanup
- `backend/crates/atc-server/src/routes.rs` — add `GET /v1/ws` route
- `backend/crates/atc-server/Cargo.toml` — add `tokio-tungstenite` to dev-dependencies
- `backend/crates/atc-server/tests/ws_tests.rs` — Tier 2 WebSocket tests using ephemeral server

**Dependencies:** Phase 1 (AppState with broadcast channel)

**Done when:** Tests verify: WS upgrade succeeds, client receives domain events after webhook ingestion, multiple clients receive same event, client disconnect is clean, lagging client continues without crash. Covers `server-wiring.AC3.*`.
<!-- END_PHASE_3 -->

<!-- START_PHASE_4 -->
### Phase 4: REST State Endpoint

**Goal:** Serve full state snapshot with `seq` cursor for client backfill on startup.

**Components:**
- `backend/crates/atc-core/src/store.rs` — add `query_all()` method returning all runs, jobs, and pool stats
- `backend/crates/atc-server/src/routes.rs` — add `GET /v1/state` handler returning `{ runs, jobs, pool_stats, seq }` where `seq` is read from `AppState.seq` at the time of the store read

**Dependencies:** Phase 1 (AppState with store)

**Done when:** Tests verify: empty state returns `seq: 0` and empty collections, state populated after webhook ingestion is reflected in response with corresponding `seq`, pool stats included. Covers `server-wiring.AC4.*`.
<!-- END_PHASE_4 -->

<!-- START_PHASE_5 -->
### Phase 5: End-to-End Integration Tests

**Goal:** Verify the full webhook → store → WebSocket and webhook → store → REST paths work together.

**Components:**
- `backend/crates/atc-server/tests/e2e_tests.rs` — Tier 3 tests using ephemeral server with real HTTP and WebSocket clients
- Test fixtures reused from `backend/crates/atc-github/tests/fixtures/`

**Dependencies:** Phases 2, 3, 4 (all endpoints functional)

**Done when:** Tests verify: POST webhook then GET /v1/state reflects the event, POST webhook then WS client receives event, multi-event sequence (run requested → job queued → job in_progress) produces correct state progression via both REST and WS. Covers `server-wiring.AC5.*`.
<!-- END_PHASE_5 -->

## Additional Considerations

**Broadcast channel capacity:** The channel needs a bounded buffer. A capacity of 256 events is reasonable for a monitoring dashboard — webhook bursts during CI runs rarely exceed tens of events per second. If a client falls more than 256 events behind, it receives `RecvError::Lagged` and should re-fetch via REST. The capacity should be a config constant, not a runtime config value (changing it requires understanding the memory/lag tradeoff).

**Seq counter lifetime:** The in-memory `AtomicU64` resets to 0 on server restart. This is consistent — the `StateStore` also starts empty on restart. Clients that reconnect after a restart will fetch a fresh snapshot with `seq: 0` and resume from there. No special restart-detection protocol is needed in Phase 9. When state externalizes to a database, the `seq` becomes a durable `BIGSERIAL` that survives restarts.

**Seq/store ordering:** The webhook handler must apply the event to the store *before* incrementing `seq` and broadcasting. This ensures that a REST snapshot at `seq: N` reflects all events up to `N`. If the order were reversed (increment seq, then apply), a concurrent `GET /v1/state` could read `seq: N` but miss the event at `N` because the store write hasn't completed yet.

**State externalization preparedness:** The design deliberately decouples the broadcast channel from the store. The REST/WS split means the read path (`GET /v1/state`) maps naturally to a DB query when state externalizes, while the WebSocket stays a thin event pipe. The in-memory `AtomicU64` maps to a `BIGSERIAL` on an event/outbox table. See GitHub issue #7 for the full externalization consideration, including domain events vs raw payloads in DB and upsert semantics.

**Documents to Update:**

| Document | Change |
|----------|--------|
| `docs/architecture/backend-server.md` | Add Server Wiring section: webhook route, WebSocket, REST state endpoint, AppState |
| `backend/crates/atc-server/CLAUDE.md` | Create: module map, contracts, test commands (follows atc-core/atc-github pattern) |
| `CLAUDE.md` (root) | Update Project Structure to reflect new modules and endpoints |
| `scripts/doc-mapping.sh` | Add mappings for new source files → architecture doc |
