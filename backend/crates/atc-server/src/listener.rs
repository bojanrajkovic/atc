//! Background tasks for PG LISTEN/NOTIFY.
//!
//! The listener task receives notifications from PG; the drain task wakes on
//! each notification, fetches outbox rows newer than a local watermark, logs
//! them, and advances the watermark. See `docs/architecture/backend-server.md`
//! for the full design.
//!
//! sqlx's `PgListener` auto-reconnects internally and re-subscribes to all
//! channels on reconnect; successful reconnects are transparent. Notifications
//! received during the brief disconnect window are healed by the drain task on
//! the next NOTIFY (it selects all rows newer than the watermark).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// The PG LISTEN/NOTIFY channel name used by the outbox.
pub(crate) const NOTIFY_CHANNEL: &str = "atc_outbox";

/// Connect a `PgListener`, subscribe to [`NOTIFY_CHANNEL`], and return it.
///
/// Extracted as a library function so the startup sequence can be tested
/// without spawning the full binary (used by AC11). Returns `Err` on connection
/// or subscription failure; callers should treat `Err` as fatal and exit.
pub async fn connect_listener(
    listener_url: &str,
) -> Result<sqlx::postgres::PgListener, sqlx::Error> {
    let mut listener = sqlx::postgres::PgListener::connect(listener_url).await?;
    listener.listen(NOTIFY_CHANNEL).await?;
    Ok(listener)
}

/// Spawn the listener task that receives PG notifications and wakes the drain task.
pub fn spawn_listener_task(
    mut listener: sqlx::postgres::PgListener,
    drain_notify: Arc<Notify>,
    cancel: CancellationToken,
    received_counter: Option<Arc<AtomicU64>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                () = cancel.cancelled() => break,
                res = listener.recv() => match res {
                    Ok(_notification) => {
                        metrics::counter!("atc_pg_notify_received_total").increment(1);
                        if let Some(c) = received_counter.as_ref() {
                            c.fetch_add(1, Ordering::Relaxed);
                        }
                        drain_notify.notify_one();
                    }
                    Err(e) => {
                        metrics::counter!("atc_pg_listener_recv_errors_total").increment(1);
                        tracing::warn!(error = %e, "pg listener recv error");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        }
    })
}

/// Spawn the drain task that fetches outbox rows on each notification and advances the watermark.
pub fn spawn_drain_task(
    pool: sqlx::PgPool,
    initial_watermark: i64,
    drain_notify: Arc<Notify>,
    cancel: CancellationToken,
    observed_passes: Option<Arc<AtomicU64>>,
    drain_started: Option<Arc<Notify>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut watermark: i64 = initial_watermark;
        loop {
            // Signal test hook: we are entering a drain pass.
            if let Some(ref s) = drain_started {
                s.notify_one();
            }
            drain_pass(&pool, &mut watermark, observed_passes.as_deref()).await;
            tokio::select! {
                () = cancel.cancelled() => break,
                () = drain_notify.notified() => {}
            }
        }
    })
}

/// Fetch outbox rows newer than `watermark`, log them, and advance the watermark.
async fn drain_pass(pool: &sqlx::PgPool, watermark: &mut i64, observed_passes: Option<&AtomicU64>) {
    let rows = sqlx::query!(
        "SELECT seq, kind, run_id, job_id FROM outbox WHERE seq > $1 ORDER BY seq",
        *watermark
    )
    .fetch_all(pool)
    .await;

    match rows {
        Ok(rows) => {
            for row in &rows {
                tracing::info!(
                    seq = row.seq,
                    kind = %row.kind,
                    run_id = row.run_id,
                    job_id = ?row.job_id,
                    "outbox drain (stub: not forwarding)"
                );
            }
            if let Some(last) = rows.last() {
                *watermark = last.seq;
            }
            metrics::counter!("atc_pg_drain_rows_total").increment(rows.len() as u64);
        }
        Err(e) => {
            tracing::warn!(error = %e, "drain_pass query failed");
        }
    }
    metrics::counter!("atc_pg_drain_passes_total").increment(1);
    if let Some(c) = observed_passes {
        c.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_channel_name_is_atc_outbox() {
        assert_eq!(NOTIFY_CHANNEL, "atc_outbox");
    }

    #[tokio::test]
    async fn connect_listener_fails_on_bad_url() {
        let result = connect_listener("postgres://nope:nope@127.0.0.1:1/x").await;
        assert!(result.is_err(), "expected Err on bad connection URL");
    }
}
