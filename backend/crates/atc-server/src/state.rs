use std::sync::Arc;

use atc_github::WebhookEvent;
use tokio::sync::{Mutex, broadcast};

use atc_core::StateStore;

/// Shared application state passed to all Axum handlers via `State<Arc<AppState>>`.
pub struct AppState {
    /// In-memory state store for workflow runs and jobs.
    pub store: Arc<StateStore>,
    /// Broadcast channel sender for pushing domain events to WebSocket clients.
    pub webhook_tx: broadcast::Sender<SeqEvent>,
    /// HMAC-SHA256 secret for verifying GitHub webhook signatures.
    /// `None` means verification is skipped.
    pub webhook_secret: Option<String>,
    /// Monotonic event counter. Incremented on each successfully ingested event.
    ///
    /// Protected by a `tokio::sync::Mutex` so that:
    /// - The webhook handler holds the lock across store mutation + seq
    ///   assignment, ensuring WS event seq order matches commit order.
    /// - The state handler holds the lock across snapshot + seq read,
    ///   ensuring the cursor matches the snapshot content.
    pub seq: Mutex<u64>,
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
    /// Snapshot of derived runner pool stats taken under the seq mutex
    /// immediately after the event applied. Populated for Job events,
    /// `None` for Run events. Clients wholesale-replace their pool state
    /// from this field when populated.
    pub pool_stats_after: Option<Vec<atc_core::RunnerPoolStats>>,
}
