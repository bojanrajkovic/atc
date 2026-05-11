//! Eviction task for the in-memory store.
//!
//! `spawn_eviction_task` wraps `InMemoryStore::evict_expired` in a supervised
//! background task following the #60 supervision pattern: biased select on
//! cancel + ticker for cooperative shutdown.

use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::in_memory::InMemoryStore;

/// Spawn a background task that periodically evicts expired entries from
/// the in-memory store.
///
/// Returns a [`JoinHandle`] that resolves when the task exits cooperatively
/// after `cancel` is cancelled.
///
/// The first eviction runs after `interval` elapses (not immediately).
/// The `cancel` arm is biased first so cancellation is always honoured
/// before the next tick fires.
pub fn spawn_eviction_task(
    store: Arc<InMemoryStore>,
    interval: Duration,
    cancel: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        // First tick completes immediately — consume it to align the cadence.
        ticker.tick().await;
        loop {
            tokio::select! {
                biased;
                () = cancel.cancelled() => break,
                _ = ticker.tick() => store.evict_expired().await,
            }
        }
    })
}
