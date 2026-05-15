//! Wire types for the WebSocket event stream and REST state snapshot.
//!
//! `CommittedEvent` is the broadcast envelope a store emits after committing
//! a write: `seq` is monotonic in commit order, `event` is the validated
//! `atc_github::WebhookEvent`. The type lives here, above the `atc-persist`
//! trait crate and the per-implementation `atc-store-{pg,mem}` store crates
//! (issue #169, ADR-0008), so the trait crate can name it without pulling in
//! `atc-github` or `serde`. ADR-0008 records the rename motivation.
//!
//! `StateSnapshot` is the REST baseline payload returned by `GET /v1/state`.

use atc_core::{Job, RunnerPoolCapacity, WorkflowRun};
use atc_github::WebhookEvent;
use serde::{Deserialize, Serialize};

/// A domain event annotated with a monotonic sequence number.
///
/// Carried over the broadcast channel and sent to WebSocket clients as JSON.
/// Clients use `seq` to reconcile the REST snapshot with the live event stream.
///
/// At the point of emission the event has been validated, applied to state,
/// and assigned a monotonic `seq` by the store's commit-order allocator —
/// hence "committed event" rather than the older "seq event" framing.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct CommittedEvent {
    /// Monotonic sequence number assigned at commit time.
    pub seq: u64,
    /// The domain event that was committed.
    pub event: WebhookEvent,
}

/// REST state snapshot for client backfill.
///
/// Returned by `GET /v1/state`. `last_seq` is the highest committed sequence
/// number — clients discard buffered WS events with `seq <= last_seq`.
/// A snapshot at `last_seq: N` reflects all committed events with event seq <= N.
///
/// `runner_pool_capacities` carries operator-declared pool ceilings (loaded
/// from the YAML config and composed into the response by
/// `routes::state_handler`, **not** by the persistent store). It is annotated
/// `#[serde(default)]` so a snapshot from an older replica that does not
/// emit the field still deserializes — the field defaults to `Vec::new()`
/// and the frontend behaves as if no capacities were declared.
#[derive(Debug, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StateSnapshot {
    pub last_seq: u64,
    pub runs: Vec<WorkflowRun>,
    pub jobs: Vec<Job>,
    #[serde(default)]
    pub runner_pool_capacities: Vec<RunnerPoolCapacity>,
}
