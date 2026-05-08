use std::sync::Arc;
use std::sync::atomic::AtomicI64;

use atc_github::WebhookEvent;
use tokio::sync::{Mutex, broadcast};

use atc_core::RunStateMachine;

use crate::persist::PersistentStore;

/// Shared application state passed to all Axum handlers via `State<Arc<AppState>>`.
pub struct AppState {
    /// In-memory state machine for workflow runs and jobs.
    ///
    /// Active in in-memory mode (`pg_pool: None`). Dormant in PG mode: the
    /// webhook handler does not write to it and `state_handler` reads from PG.
    pub state_machine: Arc<RunStateMachine>,
    /// Broadcast channel sender for pushing domain events to WebSocket clients.
    ///
    /// In in-memory mode the webhook handler writes directly. In PG mode the
    /// drain task is the sole writer; the handler is silent.
    pub webhook_tx: broadcast::Sender<SeqEvent>,
    /// HMAC-SHA256 secret for verifying GitHub webhook signatures.
    /// `None` means verification is skipped.
    pub webhook_secret: Option<String>,
    /// In-memory-mode monotonic event counter. Incremented on each successfully
    /// ingested event when `pg_pool` is `None`.
    ///
    /// Protected by a `tokio::sync::Mutex` so that:
    /// - The webhook handler holds the lock across store mutation + seq
    ///   assignment, ensuring WS event seq order matches commit order.
    /// - The state handler holds the lock across snapshot + seq read,
    ///   ensuring the cursor matches the snapshot content.
    ///
    /// In PG mode this field is unused; the seq comes from the outbox BIGSERIAL.
    ///
    /// Wrapped in `Arc` so [`crate::persist::InMemoryStore`] can hold a cloned
    /// reference and share ordering guarantees with `state_handler`.
    pub seq: Arc<Mutex<u64>>,
    /// Write-path dispatch for domain events.
    ///
    /// Routes each incoming webhook event to the appropriate backend:
    /// - [`crate::persist::PgStore`] when `pg_pool: Some(_)`.
    /// - [`crate::persist::InMemoryStore`] when `pg_pool: None`.
    ///
    /// The trait object is `Arc<dyn PersistentStore>` so it can be cloned
    /// cheaply and used across `async` handler closures.
    pub persist: Arc<dyn PersistentStore>,
    /// Optional PostgreSQL connection pool.
    ///
    /// `Some(pool)` when `ATC_DATABASE_URL` is configured; `None` runs in
    /// in-memory-only mode. Used by `/readyz`, the listener and drain tasks
    /// at startup (each takes its own clone), and the `/v1/state` REPEATABLE
    /// READ snapshot reader. Persistence writes do **not** go through this
    /// field — they dispatch through `persist` (ADR 0005).
    pub pg_pool: Option<sqlx::PgPool>,
    /// Gap-healing backstop for the outbox drain task (Phase 3c).
    ///
    /// The listener task records the lowest in-flight seq it has ever observed
    /// via `fetch_min(seq, Release)`. The drain task `swap`s this back to
    /// `i64::MAX` at pass start to capture any seq registered since the last
    /// pass; the swapped-out value lowers the SELECT floor to `min(watermark,
    /// backstop - 1)` so a delayed commit cannot leak past a stale watermark.
    /// See `docs/architecture/backend-server.md` (Drain task).
    ///
    /// Initialized to `i64::MAX` (no in-flight handlers at boot). The webhook
    /// handler does NOT touch this field — only the listener (registers) and
    /// the drain task (resets). It exists on `AppState` for spawn-argument
    /// plumbing in `main.rs`. No-op in in-memory mode.
    pub min_pending_seq: Arc<AtomicI64>,
    /// Drain-task heartbeat (epoch milliseconds, Phase 3c).
    ///
    /// The drain task stores `now_millis()` at the top of every loop iteration
    /// (whether woken by NOTIFY or by the 5s heartbeat tick) AND after every
    /// successful drain pass; failed drain passes do NOT refresh, so
    /// `/readyz` returns 503 after 30 s of sustained outbox-read failures
    /// (e.g., schema/permission drift where `SELECT 1` still succeeds).
    /// No-op in in-memory mode.
    ///
    /// Initialized to `now_millis()` at construction so `/readyz` cannot
    /// 503 between server bind and the first drain pass.
    pub last_drain_pass_at: Arc<AtomicI64>,
    /// Drain-task broadcast cursor (Phase 3c).
    ///
    /// Highest outbox `seq` the drain has fetched and broadcast through
    /// `webhook_tx`. Updated atomically after every successful drain pass.
    /// Used by `state_handler` as the PG-mode `lastSeq` because it reflects
    /// **commit order** (the drain only sees committed rows via SELECT) —
    /// `MAX(outbox.seq)` would reflect allocation order and can advance past
    /// data that hasn't materialised in a concurrent REPEATABLE READ snapshot.
    /// See ADR 0003 Phase 3c implementation notes.
    ///
    /// Initialized at boot to the same `MAX(seq)` value that seeds the
    /// drain's local `watermark`, so `/v1/state` returns a non-zero `lastSeq`
    /// before the first post-startup drain pass completes.
    pub broadcast_watermark: Arc<AtomicI64>,
}

/// A domain event annotated with a monotonic sequence number.
///
/// Carried over the broadcast channel and sent to WebSocket clients as JSON.
/// Clients use `seq` to reconcile the REST snapshot with the live event stream.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SeqEvent {
    /// Monotonic sequence number assigned at ingestion time.
    pub seq: u64,
    /// The domain event that was ingested.
    pub event: WebhookEvent,
}
