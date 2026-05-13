//! Background tasks for PG LISTEN/NOTIFY.
//!
//! The listener task receives notifications from PG and registers each NOTIFY
//! payload's seq into the gap-healing backstop atomic before waking the drain.
//! The drain task reads outbox rows newer than a local watermark, decodes each
//! row's payload, applies a bounded ring-buffer dedup, and broadcasts the
//! resulting `SeqEvent` to WS subscribers via the shared broadcast channel.
//!
//! See `docs/architecture/backend-server.md` for the full design and
//! gap-healing notes.
//!
//! sqlx's `PgListener` auto-reconnects internally and re-subscribes to all
//! channels on reconnect; successful reconnects are transparent.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use atc_github::WebhookEvent;
use tokio::sync::{Notify, broadcast};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, info_span};

use crate::metrics::PgMetrics;
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

/// Bound on the post-shutdown `COUNT(*)` query that records
/// `atc_pg_drain_shutdown_remaining_rows`. A hung database at shutdown must
/// not eat the entire `SHUTDOWN_TIMEOUT_DRAIN` budget — the count is
/// observability, not correctness, so we'd rather skip the observation than
/// delay process exit. 1 second leaves the rest of the 5 s drain shutdown
/// budget for in-flight pass cleanup.
const SHUTDOWN_REMAINING_QUERY_TIMEOUT: Duration = Duration::from_secs(1);

/// Capacity of the recently-broadcast seq ring buffer.
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
/// closes the concurrent-commits race (see `docs/architecture/backend-server.md`
/// § Gap-healing backstop).
pub fn spawn_listener_task(
    mut listener: sqlx::postgres::PgListener,
    drain_notify: Arc<Notify>,
    min_pending_seq: Arc<AtomicI64>,
    drain_in_flight: Arc<AtomicBool>,
    cancel: CancellationToken,
    received_counter: Option<Arc<AtomicU64>>,
    metrics: Arc<PgMetrics>,
) -> JoinHandle<()> {
    // Construct the task-lifetime span at spawn time. tokio::spawn does NOT
    // propagate parent spans automatically — the future is wrapped with
    // `.instrument(span)` so its descendants attach to this root.
    let task_span = info_span!("listener.task");
    tokio::spawn(
        async move {
            loop {
                tokio::select! {
                    () = cancel.cancelled() => break,
                    res = listener.recv() => match res {
                        Ok(notification) => {
                            handle_listener_notification(
                                notification,
                                drain_notify.as_ref(),
                                min_pending_seq.as_ref(),
                                drain_in_flight.as_ref(),
                                received_counter.as_deref(),
                                metrics.as_ref(),
                            );
                        }
                        Err(e) => {
                            metrics.listener_recv_errors.increment(1);
                            tracing::warn!(error = %e, "pg listener recv error");
                            tokio::time::sleep(Duration::from_secs(1)).await;
                        }
                    }
                }
            }
        }
        .instrument(task_span),
    )
}

/// Per-NOTIFY handler — extracted from `spawn_listener_task` so a `listener.recv`
/// span attaches to a single function invocation rather than a `select!` arm.
#[tracing::instrument(
    name = "listener.recv",
    skip_all,
    fields(notify.payload.seq = tracing::field::Empty),
)]
fn handle_listener_notification(
    notification: sqlx::postgres::PgNotification,
    drain_notify: &Notify,
    min_pending_seq: &AtomicI64,
    drain_in_flight: &AtomicBool,
    received_counter: Option<&AtomicU64>,
    metrics: &PgMetrics,
) {
    metrics.notify_received.increment(1);
    if let Some(c) = received_counter {
        c.fetch_add(1, Ordering::Relaxed);
    }
    // Wake-coalesce observation: if a drain pass is in
    // flight when this NOTIFY arrived, count it. Tokio's
    // Notify still collapses the permits — this counter
    // reports arrival rate vs. drain pass rate.
    if drain_in_flight.load(Ordering::Acquire) {
        metrics.wake_coalesced.increment(1);
    }
    match notification.payload().parse::<i64>() {
        Ok(seq) => {
            tracing::Span::current().record("notify.payload.seq", seq);
            let prev = min_pending_seq.fetch_min(seq, Ordering::Release);
            let new_min = prev.min(seq);
            #[allow(clippy::cast_precision_loss)]
            let gauge_value = if new_min == i64::MAX {
                f64::NAN
            } else {
                new_min as f64
            };
            metrics.min_pending_seq.set(gauge_value);
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
    startup_at: Instant,
    drain_notify: Arc<Notify>,
    min_pending_seq: Arc<AtomicI64>,
    last_drain_pass_at: Arc<AtomicI64>,
    broadcast_watermark: Arc<AtomicI64>,
    drain_in_flight: Arc<AtomicBool>,
    webhook_tx: broadcast::Sender<SeqEvent>,
    cancel: CancellationToken,
    observed_passes: Option<Arc<AtomicU64>>,
    drain_started: Option<Arc<Notify>>,
    drain_delay: Option<Duration>,
    metrics: Arc<PgMetrics>,
) -> JoinHandle<()> {
    // Construct the task-lifetime span at spawn time so descendants
    // (`drain.pass`, `drain.broadcast`) attach to this root rather than
    // becoming fresh roots.
    let task_span = info_span!("drain.task");
    tokio::spawn(
        async move {
            let shutdown_pool = pool.clone();
            let mut watermark: i64 = initial_watermark;
            let mut recent_ring: VecDeque<i64> = VecDeque::with_capacity(DEDUP_CAP);
            let mut recent_set: HashSet<i64> = HashSet::with_capacity(DEDUP_CAP);

            // Run an unconditional first pass so observed_passes/drain_started
            // fire once at startup. This preserves the test fixture invariant
            // (build_app waits for drain_started before accepting requests).
            let mut first_iter = true;
            // Startup readiness latency is observed exactly once per process,
            // after the first pass exits. See `atc_pg_drain_startup_seconds`.
            let mut startup_recorded = false;

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
                // visible before this swap is observed here. Mirror the swap into
                // the gauge: the post-swap value is i64::MAX (sentinel), rendered
                // as NaN so dashboards distinguish "no pending NOTIFY below
                // watermark" from "pending NOTIFY at seq 0".
                let backstop = min_pending_seq.swap(i64::MAX, Ordering::AcqRel);
                metrics.min_pending_seq.set(f64::NAN);
                let pass_start_floor = watermark.min(backstop.saturating_sub(1));

                // Wake-coalesce instrumentation bracket: the listener counts
                // NOTIFYs that arrive between the `store(true)` and `store(false)`
                // pair. The bracket is unconditional — if drain_pass panics,
                // Tokio terminates the task and the AtomicBool stays `true`,
                // operationally identical to "drain task is gone" so no scope
                // guard is required.
                drain_in_flight.store(true, Ordering::Release);
                let pass_start = Instant::now();
                let pass_ok = drain_pass(
                    &pool,
                    pass_start_floor,
                    &mut watermark,
                    &mut recent_ring,
                    &mut recent_set,
                    &webhook_tx,
                    drain_delay,
                    &metrics,
                )
                .await;
                drain_in_flight.store(false, Ordering::Release);
                metrics
                    .drain_pass_duration
                    .record(pass_start.elapsed().as_secs_f64());

                if !startup_recorded {
                    metrics
                        .drain_startup
                        .record(startup_at.elapsed().as_secs_f64());
                    startup_recorded = true;
                }

                metrics.drain_passes.increment(1);
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
                    // PG-mode `lastSeq` (commit-order cursor — see ADR 0003
                    // implementation notes). Using `MAX(outbox.seq)` directly is unsafe:
                    // BIGSERIAL is allocated pre-commit and can commit out of
                    // order, which would let `MAX(seq)` advance past data that
                    // hasn't materialised in a concurrent snapshot view.
                    broadcast_watermark.store(watermark, Ordering::Release);
                    #[allow(clippy::cast_precision_loss)]
                    metrics.broadcast_watermark.set(watermark as f64);
                } else {
                    // Re-register the captured backstop so the next pass still has
                    // the gap-healing floor. Without this, a transient query
                    // failure between two NOTIFYs (one carrying the low seq) would
                    // permanently lose the rescan signal: the swap zeroed the
                    // atomic and the failed pass never delivered the floor to the
                    // SELECT. Mirror the gauge alongside the atomic — the swap at
                    // pass start cleared the gauge to NaN, so without this re-mirror
                    // the gauge would advertise "drain caught up" while a pending
                    // backstop is queued for the next attempt.
                    if backstop != i64::MAX {
                        let prev = min_pending_seq.fetch_min(backstop, Ordering::Release);
                        let new_min = prev.min(backstop);
                        #[allow(clippy::cast_precision_loss)]
                        let gauge_value = if new_min == i64::MAX {
                            f64::NAN
                        } else {
                            new_min as f64
                        };
                        metrics.min_pending_seq.set(gauge_value);
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

            // Drain task is exiting. Record one shutdown observation: outbox rows
            // committed past this replica's local watermark. Validates the issue
            // #60 / #80 assumption that the outbox is unlikely to grow beyond a
            // single drain pass (DRAIN_BATCH_SIZE = 500) by the time shutdown
            // fires. Bounded by SHUTDOWN_REMAINING_QUERY_TIMEOUT so a hung DB
            // cannot stall process exit; on timeout or query error we log and
            // skip the observation rather than recording 0 (which would silently
            // mask the problem).
            record_shutdown_remaining(&shutdown_pool, watermark, &metrics).await;
        }
        .instrument(task_span),
    )
}

/// Query and record the outbox lag remaining at drain task exit.
///
/// Runs `SELECT COUNT(*) FROM outbox WHERE seq > $watermark` against the
/// drain task's own pool with a bounded timeout, observing the result into
/// `atc_pg_drain_shutdown_remaining_rows`. The observation captures rows
/// committed past `watermark` *at drain task exit time*, which is later than
/// signal arrival — the webhook handler keeps writing until axum's graceful
/// shutdown drains in-flight requests, so the count includes anything
/// committed during that window.
async fn record_shutdown_remaining(pool: &sqlx::PgPool, watermark: i64, metrics: &PgMetrics) {
    let query = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "count!: i64" FROM outbox WHERE seq > $1"#,
        watermark,
    )
    .fetch_one(pool);

    match tokio::time::timeout(SHUTDOWN_REMAINING_QUERY_TIMEOUT, query).await {
        Ok(Ok(remaining)) => {
            #[allow(clippy::cast_precision_loss)]
            metrics
                .drain_shutdown_remaining_rows
                .record(remaining as f64);
        }
        Ok(Err(e)) => {
            tracing::warn!(
                error = %e,
                watermark,
                "drain shutdown remaining-rows query failed; skipping observation",
            );
        }
        Err(_elapsed) => {
            tracing::warn!(
                watermark,
                timeout_ms = SHUTDOWN_REMAINING_QUERY_TIMEOUT.as_millis() as u64,
                "drain shutdown remaining-rows query timed out; skipping observation",
            );
        }
    }
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
#[tracing::instrument(
    name = "drain.pass",
    skip_all,
    fields(
        pass.start_floor = pass_start_floor,
        pass.rows_fetched = tracing::field::Empty,
        pass.batches = tracing::field::Empty,
    ),
)]
async fn drain_pass(
    pool: &sqlx::PgPool,
    pass_start_floor: i64,
    watermark: &mut i64,
    recent_ring: &mut VecDeque<i64>,
    recent_set: &mut HashSet<i64>,
    webhook_tx: &broadcast::Sender<SeqEvent>,
    drain_delay: Option<Duration>,
    metrics: &PgMetrics,
) -> bool {
    if let Some(d) = drain_delay {
        tokio::time::sleep(d).await;
    }
    let mut page_cursor: i64 = pass_start_floor;
    let mut max_seq_seen: Option<i64> = None;
    let mut total_rows_fetched: usize = 0;
    let mut batches: u64 = 0;

    loop {
        let rows = sqlx::query!(
            "SELECT seq, kind, payload, inserted_at FROM outbox \
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
        total_rows_fetched = total_rows_fetched.saturating_add(fetched);
        batches = batches.saturating_add(1);
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
                    metrics.drain_unknown_kind.increment(1);
                    tracing::warn!(seq = row.seq, kind = %other, "unknown outbox kind discriminator");
                    continue;
                }
            };

            if recent_set.contains(&row.seq) {
                metrics.drain_duplicate_skipped.increment(1);
            } else {
                // Event age at broadcast: now() - inserted_at, recorded once
                // per broadcast row. The metric over-reports by the writer's
                // transaction duration because `inserted_at DEFAULT now()`
                // evaluates `transaction_timestamp()` (transaction start),
                // not commit. See `metrics.md` § Operational metrics.
                #[allow(clippy::cast_precision_loss)]
                let lag_seconds = (chrono::Utc::now() - row.inserted_at)
                    .num_microseconds()
                    .unwrap_or(0) as f64
                    / 1_000_000.0;
                #[allow(clippy::cast_possible_truncation)]
                let outbox_lag_ms: i64 = (lag_seconds * 1_000.0) as i64;
                let broadcast_span = info_span!(
                    "drain.broadcast",
                    seq = row.seq,
                    kind = row.kind.as_str(),
                    outbox_lag_ms,
                );
                broadcast_span.in_scope(|| {
                    // BIGSERIAL is positive; u64 always fits.
                    let seq_u64 = u64::try_from(row.seq).unwrap_or_else(|_| {
                        tracing::error!(seq = row.seq, "negative outbox seq encountered");
                        0
                    });
                    let _ = webhook_tx.send(SeqEvent {
                        seq: seq_u64,
                        event,
                    });

                    metrics.outbox_lag.record(lag_seconds);
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
        metrics.drain_rows.increment(fetched as u64);

        if fetched < DRAIN_BATCH_SIZE as usize {
            break;
        }
    }

    let pass_span = tracing::Span::current();
    pass_span.record("pass.rows_fetched", total_rows_fetched);
    pass_span.record("pass.batches", batches);

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
        // sqlx's default connect_timeout is 30s; a 2s tokio timeout keeps this
        // unit test fast. Either branch — Err returned, or timeout fired —
        // satisfies the assertion that the bad URL did not produce a listener.
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            connect_listener("postgres://nope:nope@127.0.0.1:1/x"),
        )
        .await;
        if let Ok(Ok(_)) = result {
            panic!("expected connect_listener to fail on bad URL");
        }
    }
}
