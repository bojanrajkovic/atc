//! Background tasks for PG LISTEN/NOTIFY (Phase 2d + Phase 3c).
//!
//! The listener task receives notifications from PG and registers each NOTIFY
//! payload's seq into the gap-healing backstop atomic before waking the drain.
//! The drain task reads outbox rows newer than a local watermark, decodes each
//! row's payload, applies a bounded ring-buffer dedup, and broadcasts the
//! resulting `SeqEvent` to WS subscribers via the shared broadcast channel.
//!
//! See `docs/architecture/backend-server.md` for the full design and Phase 3c
//! gap-healing notes.
//!
//! sqlx's `PgListener` auto-reconnects internally and re-subscribes to all
//! channels on reconnect; successful reconnects are transparent.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use atc_github::WebhookEvent;
use tokio::sync::{Notify, broadcast};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::state::SeqEvent;

/// The PG LISTEN/NOTIFY channel name used by the outbox.
pub const NOTIFY_CHANNEL: &str = "atc_outbox";

/// Maximum rows fetched per drain-pass page. Bounds memory use after a long
/// listener outage; the drain loop continues until a partial page is returned.
const DRAIN_BATCH_SIZE: i64 = 500;

/// How often the drain task wakes itself even without a NOTIFY, to refresh the
/// `last_drain_pass_at` heartbeat that `/readyz` checks. 5 seconds gives a
/// 6× margin under the 30 s staleness threshold.
const HEARTBEAT_TICK: Duration = Duration::from_secs(5);

/// Capacity of the recently-broadcast seq ring buffer (Phase 3c).
///
/// At 100 webhooks/sec peak, 2048 entries cover ~20 s of drain history — orders
/// of magnitude wider than the in-flight commit window (milliseconds). Memory
/// cost: ~16 KB per replica (u64 ring + HashSet of i64).
const DEDUP_CAP: usize = 2048;

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Connect a `PgListener`, subscribe to [`NOTIFY_CHANNEL`], and return it.
pub async fn connect_listener(
    listener_url: &str,
) -> Result<sqlx::postgres::PgListener, sqlx::Error> {
    let mut listener = sqlx::postgres::PgListener::connect(listener_url).await?;
    listener.listen(NOTIFY_CHANNEL).await?;
    Ok(listener)
}

/// Spawn the listener task that receives PG notifications and wakes the drain task.
///
/// On each NOTIFY, parse the payload as `i64` (the outbox seq, emitted by
/// `notify_outbox_seq_in_txn`) and register it with `min_pending_seq` via
/// `fetch_min(seq, Release)`. The Release ordering pairs with the drain's
/// `Acquire` half of `swap(MAX, AcqRel)`, providing the synchronization that
/// closes the concurrent-commits race documented in the Phase 3c plan §D2.
pub fn spawn_listener_task(
    mut listener: sqlx::postgres::PgListener,
    drain_notify: Arc<Notify>,
    min_pending_seq: Arc<AtomicI64>,
    cancel: CancellationToken,
    received_counter: Option<Arc<AtomicU64>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                () = cancel.cancelled() => break,
                res = listener.recv() => match res {
                    Ok(notification) => {
                        metrics::counter!("atc_pg_notify_received_total").increment(1);
                        if let Some(c) = received_counter.as_ref() {
                            c.fetch_add(1, Ordering::Relaxed);
                        }
                        match notification.payload().parse::<i64>() {
                            Ok(seq) => {
                                min_pending_seq.fetch_min(seq, Ordering::Release);
                            }
                            Err(e) => {
                                tracing::warn!(
                                    payload = notification.payload(),
                                    error = %e,
                                    "malformed NOTIFY payload (expected i64 seq)",
                                );
                            }
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

/// Spawn the drain task that fetches outbox rows on each notification, decodes
/// them, applies ring-buffer dedup, and broadcasts `SeqEvent`s.
///
/// The drain task is the SOLE writer to `webhook_tx` in PG mode — the webhook
/// handler is silent. This eliminates the dual-broadcast bug that would
/// otherwise produce one event per webhook from the handler AND a second from
/// the drain.
///
/// Loop body:
/// 1. `tokio::select!` either NOTIFY arrives or 5 s heartbeat tick fires.
/// 2. Heartbeat-only wake: refresh `last_drain_pass_at` and continue (the loop
///    is alive, no DB work was attempted).
/// 3. NOTIFY-driven wake: swap `min_pending_seq` to `MAX` to capture the
///    gap-healing backstop, page through outbox rows from
///    `min(watermark, backstop - 1)` upward, decode payload, apply ring-buffer
///    dedup, broadcast on miss.
/// 4. On success: advance `watermark` and `broadcast_watermark`, refresh the
///    heartbeat, brief backoff before next iteration.
/// 5. On failure: re-register the captured backstop (otherwise a transient
///    query error would permanently lose the rescan signal) and DO NOT refresh
///    the heartbeat (so `/readyz` reflects sustained drain failure).
#[allow(clippy::too_many_arguments)]
pub fn spawn_drain_task(
    pool: sqlx::PgPool,
    initial_watermark: i64,
    drain_notify: Arc<Notify>,
    min_pending_seq: Arc<AtomicI64>,
    last_drain_pass_at: Arc<AtomicI64>,
    broadcast_watermark: Arc<AtomicI64>,
    webhook_tx: broadcast::Sender<SeqEvent>,
    cancel: CancellationToken,
    observed_passes: Option<Arc<AtomicU64>>,
    drain_started: Option<Arc<Notify>>,
    drain_delay: Option<Duration>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut watermark: i64 = initial_watermark;
        let mut recent_ring: VecDeque<i64> = VecDeque::with_capacity(DEDUP_CAP);
        let mut recent_set: HashSet<i64> = HashSet::with_capacity(DEDUP_CAP);

        // Run an unconditional first pass so observed_passes/drain_started
        // fire once at startup. This preserves the test fixture invariant
        // (build_app waits for drain_started) and matches Phase 2d behavior.
        let mut first_iter = true;

        loop {
            let woken_by_notify = if first_iter {
                first_iter = false;
                true
            } else {
                tokio::select! {
                    () = cancel.cancelled() => break,
                    () = drain_notify.notified() => true,
                    () = tokio::time::sleep(HEARTBEAT_TICK) => false,
                }
            };

            if !woken_by_notify {
                // Heartbeat-only wake: refresh /readyz and continue. No drain
                // work was attempted, so this is not a "drain succeeded" signal
                // — it's a "loop is alive". Updating the timestamp here is
                // correct because the alternative (only refresh on successful
                // drain) would 503 quiet replicas after 30 s.
                last_drain_pass_at.store(now_millis(), Ordering::Relaxed);
                continue;
            }

            // Capture the gap-healing backstop and reset the atomic. AcqRel
            // pairs with the listener's Release fetch_min so any registration
            // visible before this swap is observed here.
            let backstop = min_pending_seq.swap(i64::MAX, Ordering::AcqRel);
            let pass_start_floor = watermark.min(backstop.saturating_sub(1));

            let pass_ok = drain_pass(
                &pool,
                pass_start_floor,
                &mut watermark,
                &mut recent_ring,
                &mut recent_set,
                &webhook_tx,
                drain_delay,
            )
            .await;

            metrics::counter!("atc_pg_drain_passes_total").increment(1);
            if let Some(c) = observed_passes.as_deref() {
                c.fetch_add(1, Ordering::Relaxed);
            }
            if let Some(ref s) = drain_started {
                s.notify_one();
            }

            if pass_ok {
                // Refresh heartbeat ONLY on success. A failed drain pass must
                // not advertise readiness — after 30 s of failures, /readyz
                // will go stale and traffic gets routed away.
                last_drain_pass_at.store(now_millis(), Ordering::Relaxed);
                // Publish the broadcast cursor. Read by `state_handler` as the
                // PG-mode `lastSeq` (commit-order cursor — see ADR 0003 Phase
                // 3c notes). Using `MAX(outbox.seq)` directly is unsafe:
                // BIGSERIAL is allocated pre-commit and can commit out of
                // order, which would let `MAX(seq)` advance past data that
                // hasn't materialised in a concurrent snapshot view.
                broadcast_watermark.store(watermark, Ordering::Release);
            } else {
                // Re-register the captured backstop so the next pass still has
                // the gap-healing floor. Without this, a transient query
                // failure between two NOTIFYs (one carrying the low seq) would
                // permanently lose the rescan signal: the swap zeroed the
                // atomic and the failed pass never delivered the floor to the
                // SELECT.
                if backstop != i64::MAX {
                    min_pending_seq.fetch_min(backstop, Ordering::Release);
                }
                // Force the next iteration to attempt another drain pass.
                // Without this, the loop would re-enter the `tokio::select!`
                // and — if no new webhook arrives — wait on the 5 s heartbeat
                // tick, take the heartbeat-only arm, refresh the timestamp,
                // and `continue` without ever retrying the drain. A single
                // committed row pending after a transient query failure could
                // then sit undelivered indefinitely while `/readyz` stayed
                // healthy via heartbeat ticks. `notify_one()` adds a permit
                // so the next `drain_notify.notified()` resolves immediately
                // and the next iteration runs as a NOTIFY-driven pass.
                drain_notify.notify_one();
                // Brief backoff before retry. Don't refresh heartbeat — that's
                // the entire point of this branch.
                tokio::select! {
                    () = cancel.cancelled() => break,
                    () = tokio::time::sleep(Duration::from_secs(1)) => {}
                }
            }
        }
    })
}

/// Page through outbox rows from `pass_start_floor` upward, decoding payload,
/// applying ring-buffer dedup, broadcasting `SeqEvent` on miss.
///
/// On success advances `watermark` to the highest seq seen (or leaves it
/// unchanged if no rows were fetched). Returns `false` on any query error.
///
/// `page_cursor` is local to this function and tracks the highest seq seen so
/// far in the current pass; pagination uses `page_cursor` (not `watermark`) so
/// a backstop-lowered rescan does not skip rows when the floor is below the
/// pre-existing watermark.
#[allow(clippy::too_many_arguments)]
async fn drain_pass(
    pool: &sqlx::PgPool,
    pass_start_floor: i64,
    watermark: &mut i64,
    recent_ring: &mut VecDeque<i64>,
    recent_set: &mut HashSet<i64>,
    webhook_tx: &broadcast::Sender<SeqEvent>,
    drain_delay: Option<Duration>,
) -> bool {
    if let Some(d) = drain_delay {
        tokio::time::sleep(d).await;
    }
    let mut page_cursor: i64 = pass_start_floor;
    let mut max_seq_seen: Option<i64> = None;

    loop {
        let rows = sqlx::query!(
            "SELECT seq, kind, payload FROM outbox \
             WHERE seq > $1 ORDER BY seq LIMIT $2",
            page_cursor,
            DRAIN_BATCH_SIZE,
        )
        .fetch_all(pool)
        .await;

        let rows = match rows {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "drain_pass query failed");
                return false;
            }
        };

        if rows.is_empty() {
            break;
        }

        let fetched = rows.len();
        for row in &rows {
            // Advance pagination state at the top of the loop so a `continue`
            // on decode/discriminator failures still moves the page cursor.
            // Otherwise a single bad row would cause `SELECT > page_cursor` to
            // re-fetch the same batch forever — the in-memory dedup ring would
            // suppress duplicate broadcasts but the DB query would loop until
            // the channel buffer fills or the test times out.
            page_cursor = row.seq;
            max_seq_seen = Some(max_seq_seen.unwrap_or(row.seq).max(row.seq));

            let event: WebhookEvent = match row.kind.as_str() {
                "run" => match serde_json::from_value(row.payload.clone()) {
                    Ok(env) => WebhookEvent::Run(env),
                    Err(e) => {
                        tracing::error!(
                            seq = row.seq,
                            error = %e,
                            "failed to decode run outbox payload",
                        );
                        continue;
                    }
                },
                "job" => match serde_json::from_value(row.payload.clone()) {
                    Ok(env) => WebhookEvent::Job(env),
                    Err(e) => {
                        tracing::error!(
                            seq = row.seq,
                            error = %e,
                            "failed to decode job outbox payload",
                        );
                        continue;
                    }
                },
                other => {
                    metrics::counter!("atc_pg_drain_unknown_kind_total").increment(1);
                    tracing::warn!(seq = row.seq, kind = %other, "unknown outbox kind discriminator");
                    continue;
                }
            };

            if recent_set.contains(&row.seq) {
                metrics::counter!("atc_pg_drain_duplicate_skipped_total").increment(1);
            } else {
                // BIGSERIAL is positive; u64 always fits.
                let seq_u64 = u64::try_from(row.seq).unwrap_or_else(|_| {
                    tracing::error!(seq = row.seq, "negative outbox seq encountered");
                    0
                });
                let _ = webhook_tx.send(SeqEvent {
                    seq: seq_u64,
                    event,
                });

                recent_ring.push_back(row.seq);
                recent_set.insert(row.seq);
                if recent_ring.len() > DEDUP_CAP
                    && let Some(evicted) = recent_ring.pop_front()
                {
                    recent_set.remove(&evicted);
                }
            }
        }
        metrics::counter!("atc_pg_drain_rows_total").increment(fetched as u64);

        if fetched < DRAIN_BATCH_SIZE as usize {
            break;
        }
    }

    if let Some(seen) = max_seq_seen {
        *watermark = (*watermark).max(seen);
    }
    true
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
