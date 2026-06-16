//! Determinism tests for the wall-clock seam threaded through `PgStore`.
//!
//! `PgStore` reads wall-clock time via an `Arc<dyn Clock>` it owns; the drain
//! task uses it for the heartbeat refresh, `liveness_check` uses it for the
//! staleness comparison, and `drain_pass` uses it for the outbox-lag
//! observation. With a `TestClock` substituted at construction, each of these
//! values becomes exactly reproducible across runs — these tests pin that
//! contract.

use crate::common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use atc_core::{Clock, TestClock, fixed_test_timestamp};
use atc_persist::LivenessError;
use chrono::TimeDelta;
use serial_test::serial;
use tokio::time::timeout;

/// Advancing the `TestClock` 31 s past the recorded heartbeat drives
/// `liveness_check()` to return `DrainStale { age_ms: 31_000 }` exactly.
///
/// The drain task's unconditional first pass (see `listener.rs:222-224`)
/// records `clock.now()` into `last_drain_pass_at`. We wait for that pass to
/// complete via `drain_started`, then abort the drain so no later iteration
/// can refresh the heartbeat. Advancing the test clock by exactly 31 s makes
/// the staleness age `31_000` — over the 30 s threshold and bit-for-bit
/// reproducible.
#[tokio::test]
#[serial]
async fn liveness_check_reports_drain_stale_after_31s_under_test_clock() {
    common::ensure_recorder_installed();

    let (pool, _container, db_url) = common::start_pg().await;

    let clock = Arc::new(TestClock::new(fixed_test_timestamp()));
    let fixture =
        common::build_app_with_pg_clock(Arc::clone(&clock) as Arc<dyn Clock>, pool, db_url).await;

    // The startup pass already fired `drain_started` (see
    // `build_app_with_pg_clock`), so `last_drain_pass_at` holds
    // `fixed_test_timestamp().timestamp_millis()` exactly. Abort the drain so
    // no further iteration refreshes the heartbeat.
    fixture.drain_abort.abort();
    // Give Tokio a beat to observe the cancel at the next await point.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Sanity-check the recorded heartbeat — should equal `fixed_test_timestamp`
    // in milliseconds.
    let heartbeat_ms = fixture.last_drain_pass_at.load(Ordering::Relaxed);
    assert_eq!(
        heartbeat_ms,
        fixed_test_timestamp().timestamp_millis(),
        "drain startup pass should record clock.now() into the heartbeat",
    );

    // Advance the test clock past the 30 s staleness threshold.
    clock.advance(TimeDelta::seconds(31));

    let result = fixture.state.persist.liveness_check().await;
    match result {
        Err(LivenessError::DrainStale { age_ms }) => {
            assert_eq!(
                age_ms, 31_000,
                "TestClock advance should produce exact age_ms; got {age_ms}",
            );
        }
        Err(other) => panic!("expected DrainStale, got {other:?}"),
        Ok(()) => panic!("expected DrainStale, got Ok"),
    }

    fixture.shutdown.cancel();
}

/// A single outbox row whose `inserted_at = fixed_test_timestamp()` and a
/// `TestClock` advanced by exactly 5 s produces an `atc_pg_outbox_lag_seconds`
/// observation with `count == 1` and `sum == 5.0`.
///
/// This locks the determinism of the lag metric: production reads
/// `clock.now() - row.inserted_at` in the broadcast site, and with a
/// `TestClock` both sides of that subtraction are now fully controllable.
#[tokio::test]
#[serial]
async fn outbox_lag_is_deterministic_under_test_clock() {
    const METRIC: &str = "atc_pg_outbox_lag_seconds";

    common::ensure_recorder_installed();
    common::reset_metrics();

    let (pool, _container, db_url) = common::start_pg().await;

    let clock = Arc::new(TestClock::new(fixed_test_timestamp()));
    let fixture =
        common::build_app_with_pg_clock(Arc::clone(&clock) as Arc<dyn Clock>, pool.clone(), db_url)
            .await;

    // The startup pass already happened; reset metrics so its (empty-outbox)
    // pass cannot bleed into the histogram count we are about to assert on.
    common::reset_metrics();

    let baseline_passes = fixture.observed_passes.load(Ordering::Relaxed);

    // Insert a stub run row to satisfy the outbox FK (`fk_outbox_runs`).
    let run_id: i64 = 70_000_000_001;
    sqlx::query(
        "INSERT INTO runs (id, org, repo, head_sha, event, display_title, html_url, \
         status, created_at, updated_at, placeholder) \
         VALUES ($1, 'test', 'test', '', '', '', '', 'Queued', $2, $2, true)",
    )
    .bind(run_id)
    .bind(fixed_test_timestamp())
    .execute(&pool)
    .await
    .expect("stub run insert failed");

    // Construct a valid `RunEventEnvelope` so the drain decode path succeeds
    // and the row reaches the broadcast site.
    let fixture_bytes = common::fixture_workflow_run_requested();
    let base_env = match atc_github::parse_webhook("workflow_run", &fixture_bytes)
        .expect("fixture must parse")
    {
        atc_github::ParseResult::Parsed(ev) => match *ev {
            atc_github::WebhookEvent::Run(e) => e,
            _ => panic!("expected Run variant"),
        },
        other => panic!("fixture must parse to Run, got {other:?}"),
    };
    let mut env = base_env.clone();
    env.run_id = atc_core::types::RunId(run_id);
    let payload = serde_json::to_value(&env).expect("env serialization failed");

    // Bind `inserted_at` explicitly so PG's wall-clock `DEFAULT now()` does
    // NOT fire — otherwise the lag value is real wall-clock and the test is
    // non-deterministic.
    let inserted_seq: i64 = sqlx::query_scalar(
        "INSERT INTO outbox (kind, run_id, payload, inserted_at) \
         VALUES ('run', $1, $2, $3) RETURNING seq",
    )
    .bind(run_id)
    .bind(&payload)
    .bind(fixed_test_timestamp())
    .fetch_one(&pool)
    .await
    .expect("explicit-inserted_at outbox INSERT failed");

    // Advance the TestClock by exactly 5 s. Production reads
    // `clock.now() - row.inserted_at` at the broadcast site.
    clock.advance(TimeDelta::seconds(5));

    // Manual NOTIFY wakes the drain through the listener — the listener
    // registers the seq into `min_pending_seq` and calls `notify_one()` on
    // `drain_notify`. The drain reads the row, hits the broadcast site, and
    // records the lag observation.
    sqlx::query("SELECT pg_notify('atc_outbox', $1::text)")
        .bind(inserted_seq)
        .execute(&pool)
        .await
        .expect("manual NOTIFY failed");

    timeout(Duration::from_secs(5), async {
        loop {
            tokio::time::sleep(Duration::from_millis(30)).await;
            if fixture.observed_passes.load(Ordering::Relaxed) > baseline_passes {
                return;
            }
        }
    })
    .await
    .expect("drain did not process the row within 5s");

    let snapshot = common::snapshot_metrics();
    let count = common::histogram_count(&snapshot, METRIC, &[]);
    let sum = common::histogram_sum(&snapshot, METRIC, &[]);

    assert_eq!(
        count, 1,
        "expected exactly one outbox-lag observation per broadcast row; got {count}",
    );
    assert!(
        (sum - 5.0).abs() < 1e-9,
        "TestClock advance of 5s should produce sum == 5.0; got {sum}",
    );

    fixture.shutdown.cancel();
}
