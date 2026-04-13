use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use atc_github::WebhookEvent;
use tokio::sync::broadcast;

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
    pub seq: AtomicU64,
}

/// A domain event annotated with a monotonic sequence number.
///
/// Carried over the broadcast channel and sent to WebSocket clients as JSON.
/// Clients use `seq` to reconcile the REST snapshot with the live event stream.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SeqEvent {
    /// Monotonic sequence number assigned at ingestion time.
    pub seq: u64,
    /// The domain event that was ingested.
    pub event: WebhookEvent,
}
