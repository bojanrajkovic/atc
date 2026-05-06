//! Phase 3c integration tests: gap-healing backstop and dedup ring buffer.
//!
//! T6  — Drain dedup: reverse-order concurrent-commit A/B race from §D2.
//!        Transaction B commits first (seq N+1), drain broadcasts it.
//!        Transaction A commits late (seq N), triggering a backstop rescan.
//!        Rescan picks up both rows; seq N+1 is suppressed by dedup ring,
//!        seq N is new and gets broadcast. Dedup counter increments exactly 1.
//! T6b — Drain pagination: a rescan triggered after seeding >DRAIN_BATCH_SIZE
//!        rows is paginated correctly (all rows forwarded, no duplicates).
//! T7  — min_pending_seq swap unit test: pure Rust, no testcontainers.
//!
//! Docker/OrbStack required for T6 and T6b.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use serial_test::serial;
use tokio::time::timeout;

// ---------------------------------------------------------------------------
// Local metric-parsing helpers (copied from outbox_tests.rs to avoid coupling)
// ---------------------------------------------------------------------------

/// Render Prometheus metrics as a string. Thin wrapper kept sync to match usage.
fn render_metrics() -> String {
    common::render_metrics()
}

/// Parse an unlabeled counter (no `{...}` label set) from Prometheus text output.
fn parse_unlabeled_counter(metrics_body: &str, name: &str) -> u64 {
    for line in metrics_body.lines() {
        if line.starts_with('#') {
            continue;
        }
        if line.starts_with(name)
            && line[name.len()..].starts_with(char::is_whitespace)
            && let Some(value_str) = line.split_whitespace().last()
        {
            return value_str.parse::<u64>().unwrap_or(0);
        }
    }
    0
}

// ─── T7: pure unit test for min_pending_seq swap semantics ──────────────────

/// T7: Verify the gap-healing backstop atomic swap behavior.
///
/// `fetch_min(seq, Release)` registers the pending seq from a NOTIFY. The drain
/// then `swap(MAX, AcqRel)` to capture it. This tests the invariant directly
/// without any PG dependency.
#[test]
fn phase_3c_gap_healing_t7_min_pending_seq_swap_semantics() {
    let atomic = Arc::new(AtomicI64::new(i64::MAX));

    // Simulate listener receiving NOTIFY for seq=5.
    let prev = atomic.fetch_min(5, Ordering::Release);
    // Should have swapped from MAX down to 5.
    assert_eq!(
        prev,
        i64::MAX,
        "fetch_min should return the old value (MAX)"
    );
    assert_eq!(
        atomic.load(Ordering::Acquire),
        5,
        "atomic should now hold 5 (the min)"
    );

    // Simulate a second NOTIFY for seq=3 (earlier seq, so min drops further).
    let prev2 = atomic.fetch_min(3, Ordering::Release);
    assert_eq!(prev2, 5, "second fetch_min returns old value (5)");
    assert_eq!(
        atomic.load(Ordering::Acquire),
        3,
        "atomic should now hold 3"
    );

    // Simulate drain swapping backstop to MAX and capturing the floor.
    let captured = atomic.swap(i64::MAX, Ordering::AcqRel);
    assert_eq!(captured, 3, "swap should return 3 (the captured backstop)");
    assert_eq!(
        atomic.load(Ordering::Acquire),
        i64::MAX,
        "atomic should be reset to MAX after swap"
    );

    // A NOTIFY after the swap for seq=7 should register correctly.
    atomic.fetch_min(7, Ordering::Release);
    assert_eq!(
        atomic.load(Ordering::Acquire),
        7,
        "new NOTIFY after reset should register"
    );

    // Another drain sweep: capture and reset.
    let captured2 = atomic.swap(i64::MAX, Ordering::AcqRel);
    assert_eq!(captured2, 7, "second swap captures 7");
    assert_eq!(
        atomic.load(Ordering::Acquire),
        i64::MAX,
        "reset to MAX again"
    );
}

/// T7b: fetch_min does not go below an already-smaller value.
#[test]
fn phase_3c_gap_healing_t7b_fetch_min_does_not_increase() {
    let atomic = Arc::new(AtomicI64::new(10));

    // Attempting to register seq=20 should not change the stored minimum.
    let prev = atomic.fetch_min(20, Ordering::Release);
    assert_eq!(prev, 10, "prev should be 10 (the minimum)");
    assert_eq!(
        atomic.load(Ordering::Acquire),
        10,
        "stored value stays 10 — fetch_min is monotone-decreasing"
    );
}

// ─── T6: dedup ring buffer suppresses re-broadcast in real A/B race ─────────

/// T6: Two raw concurrent SQL transactions manufacture the §D2 reverse-order
///     concurrent-commit race: B commits first (seq N+1), drain broadcasts it
///     and advances the watermark past N. A commits late (seq N), its deferred
///     NOTIFY fires, backstop captures N, rescan fetches both N and N+1.
///     seq N+1 is in the dedup ring → suppressed (counter +1). seq N is new →
///     broadcast. The test asserts both seqs arrive, B before A, and that the
///     duplicate-skip counter increments by exactly 1 during the rescan pass.
///
/// Implementation note on NOTIFY timing: `SELECT pg_notify(...)` inside an
/// open transaction queues the notification; PG delivers it on COMMIT. So A's
/// NOTIFY does not fire until A commits in step 6 — the listener only wakes the
/// drain for seq_a after that point.
#[tokio::test]
#[serial]
async fn phase_3c_gap_healing_t6_dedup_suppresses_rescan_rebroadcast() {
    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool.clone(), db_url).await;

    // Subscribe a fresh broadcast receiver for seq-observation.
    let mut rx = fixture.state.webhook_tx.subscribe();

    // ── Step 1: Pre-insert two stub run rows to satisfy outbox FK ─────────────
    // Use run_id values unlikely to collide with fixture constants. We need
    // placeholder=true because we're inserting directly without the real handler.
    sqlx::query(
        "INSERT INTO runs (id, org, repo, head_sha, event, display_title, html_url, \
         status, created_at, updated_at, placeholder) \
         VALUES (42000000001, 'test', 'test', '', '', '', '', 'Queued', NOW(), NOW(), true)",
    )
    .execute(&pool)
    .await
    .expect("stub run 42000000001 insert failed");

    sqlx::query(
        "INSERT INTO runs (id, org, repo, head_sha, event, display_title, html_url, \
         status, created_at, updated_at, placeholder) \
         VALUES (42000000002, 'test', 'test', '', '', '', '', 'Queued', NOW(), NOW(), true)",
    )
    .execute(&pool)
    .await
    .expect("stub run 42000000002 insert failed");

    // ── Build valid envelope JSON for each run_id ─────────────────────────────
    // Parse the fixture → mutate run_id → serialize back as outbox JSONB.
    let fixture_bytes = common::fixture_workflow_run_requested();
    let base_env = match atc_github::parse_webhook("workflow_run", &fixture_bytes)
        .expect("fixture must parse")
    {
        atc_github::ParseResult::Parsed(ev) => match *ev {
            atc_github::WebhookEvent::Run(e) => e,
            _ => panic!("expected Run variant"),
        },
        atc_github::ParseResult::Skipped { .. } => panic!("fixture must not be skipped"),
    };

    let mut env_a = base_env.clone();
    env_a.run_id = atc_core::types::RunId(42_000_000_001);
    let payload_a = serde_json::to_value(&env_a).expect("env_a serialization failed");

    let mut env_b = base_env.clone();
    env_b.run_id = atc_core::types::RunId(42_000_000_002);
    let payload_b = serde_json::to_value(&env_b).expect("env_b serialization failed");

    // ── Step 3: Open transaction A, INSERT outbox row, pg_notify (don't commit) ─
    // Sequentially allocate seq_a then seq_b so seq_a < seq_b is deterministic.
    let mut tx_a = pool.begin().await.expect("begin tx_a failed");

    let seq_a: i64 = sqlx::query_scalar(
        "INSERT INTO outbox (kind, run_id, payload) VALUES ('run', 42000000001, $1) RETURNING seq",
    )
    .bind(&payload_a)
    .fetch_one(&mut *tx_a)
    .await
    .expect("outbox INSERT A failed");

    sqlx::query("SELECT pg_notify('atc_outbox', $1::text)")
        .bind(seq_a)
        .execute(&mut *tx_a)
        .await
        .expect("pg_notify A queued");

    // ── Step 4: Open transaction B, INSERT outbox row, pg_notify, COMMIT ────────
    let mut tx_b = pool.begin().await.expect("begin tx_b failed");

    let seq_b: i64 = sqlx::query_scalar(
        "INSERT INTO outbox (kind, run_id, payload) VALUES ('run', 42000000002, $1) RETURNING seq",
    )
    .bind(&payload_b)
    .fetch_one(&mut *tx_b)
    .await
    .expect("outbox INSERT B failed");

    sqlx::query("SELECT pg_notify('atc_outbox', $1::text)")
        .bind(seq_b)
        .execute(&mut *tx_b)
        .await
        .expect("pg_notify B queued");

    tx_b.commit().await.expect("tx_b commit failed");

    // ── Step 5: Wait for seq_b to land on the broadcast receiver ─────────────
    // Drain woke on B's NOTIFY, processed seq_b, watermark advanced past seq_a.
    let seq_b_u64 = u64::try_from(seq_b).expect("seq_b must be positive");
    timeout(Duration::from_secs(10), async {
        loop {
            match rx.recv().await {
                Ok(ev) if ev.seq == seq_b_u64 => return,
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    })
    .await
    .expect("timed out waiting for seq_b broadcast (B committed first)");

    // ── Capture dedup baseline BEFORE committing A ────────────────────────────
    let baseline_dup =
        parse_unlabeled_counter(&render_metrics(), "atc_pg_drain_duplicate_skipped_total");

    // Record pass count before committing A.
    let passes_before = fixture.observed_passes.load(Ordering::Relaxed);

    // ── Step 6: Commit A — deferred NOTIFY fires, drain rescans ──────────────
    tx_a.commit().await.expect("tx_a commit failed");

    // ── Step 7: Wait for seq_a to arrive on the receiver ─────────────────────
    // Rescan floor = min(watermark, seq_a - 1) < seq_b, so both rows returned.
    // seq_b suppressed by dedup ring; seq_a new → broadcast.
    let seq_a_u64 = u64::try_from(seq_a).expect("seq_a must be positive");
    timeout(Duration::from_secs(10), async {
        loop {
            match rx.recv().await {
                Ok(ev) if ev.seq == seq_a_u64 => return,
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    })
    .await
    .expect("timed out waiting for seq_a broadcast after tx_a commit");

    // ── Step 8: Wait for the rescan pass to fully complete ───────────────────
    timeout(Duration::from_secs(5), async {
        loop {
            tokio::time::sleep(Duration::from_millis(30)).await;
            if fixture.observed_passes.load(Ordering::Relaxed) > passes_before {
                return;
            }
        }
    })
    .await
    .expect("drain did not complete the rescan pass within 5s");

    // ── Assertions ────────────────────────────────────────────────────────────
    // Dedup counter must have incremented by exactly 1 (seq_b suppressed once).
    let after_dup =
        parse_unlabeled_counter(&render_metrics(), "atc_pg_drain_duplicate_skipped_total");
    assert_eq!(
        after_dup,
        baseline_dup + 1,
        "dedup ring must suppress seq_b exactly once during rescan; \
         baseline={baseline_dup} after={after_dup}"
    );

    fixture.shutdown.cancel();
}

// ─── T6b: drain pagination ──────────────────────────────────────────────────

/// T6b: The drain paginates across `DRAIN_BATCH_SIZE` correctly and
///      `atc_pg_drain_rows_total` advances by exactly 600 during the rescan.
///
/// Design:
///   1. Pre-seed 600 outbox rows with valid `RunEventEnvelope` JSON BEFORE
///      booting the fixture, so `initial_watermark = MAX(seq) = 600`.
///   2. Boot fixture. First unconditional drain pass: floor = 600 →
///      `SELECT > 600` returns 0 rows → `atc_pg_drain_rows_total` unchanged.
///   3. Capture `atc_pg_drain_rows_total` baseline.
///   4. Force a backstop-driven rescan: emit `pg_notify('atc_outbox', '1')`.
///      Listener calls `fetch_min(1)`. Drain wakes, backstop=1 →
///      `pass_start_floor = min(600, 0) = 0` → `SELECT > 0` returns 600 rows
///      in 2 batches (500 + 100).
///   5. Wait for `observed_passes` to advance (rescan complete).
///   6. Assert `atc_pg_drain_rows_total - baseline == 600`.
///
/// The DRAIN_BATCH_SIZE is 500, so a 600-row DB forces 2 pagination iterations.
/// The counter assertion proves the loop did NOT stop at the 500-row boundary.
#[tokio::test]
#[serial]
async fn phase_3c_gap_healing_t6b_drain_paginates_across_batch_boundary() {
    let (pool, _container, db_url) = common::start_pg().await;

    // ── Step 1: Pre-seed 600 outbox rows with VALID payloads ─────────────────
    // Parse the fixture → get a real RunEventEnvelope → mutate run_id per row
    // → serialize as JSONB so the drain can fully decode each row.
    sqlx::query(
        "INSERT INTO runs (id, org, repo, head_sha, event, display_title, html_url, \
         status, created_at, updated_at, placeholder) \
         VALUES (40000000010, 'test', 'test', '', '', '', '', 'Queued', NOW(), NOW(), true)",
    )
    .execute(&pool)
    .await
    .expect("stub run insert failed");

    let fixture_bytes = common::fixture_workflow_run_requested();
    let base_env = match atc_github::parse_webhook("workflow_run", &fixture_bytes)
        .expect("fixture must parse")
    {
        atc_github::ParseResult::Parsed(ev) => match *ev {
            atc_github::WebhookEvent::Run(e) => e,
            _ => panic!("expected Run variant"),
        },
        atc_github::ParseResult::Skipped { .. } => panic!("fixture must not be skipped"),
    };

    for i in 0i64..600 {
        let mut env = base_env.clone();
        // Keep run_id at the single stub row so the FK is always satisfied.
        env.run_id = atc_core::types::RunId(40_000_000_010);
        let payload = serde_json::to_value(&env).expect("env serialization failed");
        sqlx::query("INSERT INTO outbox (kind, run_id, payload) VALUES ('run', 40000000010, $1)")
            .bind(&payload)
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("outbox seed row {i} failed: {e}"));
    }

    // Verify 600 rows seeded.
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM outbox")
        .fetch_one(&pool)
        .await
        .expect("count query failed");
    assert_eq!(count, 600, "should have seeded 600 outbox rows before boot");

    // ── Step 2: Boot fixture ─────────────────────────────────────────────────
    // initial_watermark = COALESCE(MAX(seq), 0) = 600.
    // First unconditional pass: floor = 600 → SELECT > 600 = 0 rows.
    let fixture = common::build_app_with_pg_and_listener(pool.clone(), db_url).await;

    let passes_after_startup = fixture.observed_passes.load(Ordering::Relaxed);
    assert!(
        passes_after_startup >= 1,
        "at least one pass must have completed at startup (got {passes_after_startup})"
    );

    // ── Step 3: Capture rows_total baseline ──────────────────────────────────
    let baseline_rows = parse_unlabeled_counter(&render_metrics(), "atc_pg_drain_rows_total");

    // ── Step 4: Force backstop-driven rescan via manual NOTIFY ───────────────
    // NOTIFY '1' → listener fetch_min(1) → drain backstop=1 →
    // pass_start_floor = min(600, 0) = 0 → SELECT > 0 returns 600 rows.
    sqlx::query("SELECT pg_notify('atc_outbox', '1')")
        .execute(&pool)
        .await
        .expect("manual NOTIFY failed");

    // ── Step 5: Wait for drain to complete the rescan pass ───────────────────
    timeout(Duration::from_secs(30), async {
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let passes_now = fixture.observed_passes.load(Ordering::Relaxed);
            if passes_now > passes_after_startup {
                return;
            }
        }
    })
    .await
    .expect("drain did not complete the rescan pass within 30s");

    // ── Step 6: Assert rows_total delta == 600 ────────────────────────────────
    let after_rows = parse_unlabeled_counter(&render_metrics(), "atc_pg_drain_rows_total");
    assert_eq!(
        after_rows - baseline_rows,
        600,
        "rescan must process exactly 600 rows (2 batches: 500+100); \
         baseline={baseline_rows} after={after_rows}"
    );

    fixture.shutdown.cancel();
}

// ─── T8: drain silently skips bogus-payload rows ────────────────────────────

/// T8: Drain payload-decode error paths: `kind='run'` and `kind='job'` rows
///     with payloads that do not deserialize as the expected envelope types.
///
/// The CHECK constraint on `outbox.kind` is `kind IN ('run', 'job')`, so the
/// unknown-kind discriminator path in `drain_pass` is unreachable from
/// constrained SQL. We therefore only exercise the two bogus-payload paths.
/// The unknown-kind counter (`atc_pg_drain_unknown_kind_total`) is expected to
/// stay at its baseline (0 additional increments).
///
/// Design:
/// 1. Boot fixture (empty DB → initial watermark = 0, first pass sees 0 rows).
/// 2. Subscribe a fresh broadcast receiver AFTER boot so the startup pass
///    produces no events on this receiver.
/// 3. Insert a stub run row to satisfy outbox FK.
/// 4. Insert TWO bad outbox rows directly via SQL:
///    - kind='run', payload='{"bogus":true}'::jsonb  (won't decode as RunEventEnvelope)
///    - kind='job', payload='{"bogus":true}'::jsonb  (won't decode as JobEventEnvelope)
/// 5. Force a rescan via `SELECT pg_notify('atc_outbox', '1')`.
/// 6. Wait for drain to advance observed_passes (pass completed).
/// 7. Assert `rx.try_recv()` returns Empty (no broadcasts emitted for bad rows).
/// 8. Assert metric counters:
///    - `atc_pg_drain_rows_total` advanced by 2.
///    - `atc_pg_drain_unknown_kind_total` unchanged (no unknown-kind rows inserted).
///    - `atc_pg_drain_duplicate_skipped_total` unchanged.
#[tokio::test]
#[serial]
async fn phase_3c_t8_drain_skips_bogus_payload_rows() {
    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool.clone(), db_url).await;

    let passes_after_startup = fixture.observed_passes.load(Ordering::Relaxed);

    // Subscribe AFTER boot so the startup pass (0 rows) doesn't noise the receiver.
    let mut rx = fixture.state.webhook_tx.subscribe();

    // ── Capture metric baselines ─────────────────────────────────────────────
    let baseline_rows = parse_unlabeled_counter(&render_metrics(), "atc_pg_drain_rows_total");
    let baseline_dup =
        parse_unlabeled_counter(&render_metrics(), "atc_pg_drain_duplicate_skipped_total");
    let baseline_unknown =
        parse_unlabeled_counter(&render_metrics(), "atc_pg_drain_unknown_kind_total");

    // ── Insert stub run row to satisfy outbox FK ──────────────────────────────
    sqlx::query(
        "INSERT INTO runs (id, org, repo, head_sha, event, display_title, html_url, \
         status, created_at, updated_at, placeholder) \
         VALUES (50000000001, 'test', 'test', '', '', '', '', 'Queued', NOW(), NOW(), true)",
    )
    .execute(&pool)
    .await
    .expect("stub run insert failed");

    // ── Insert two bad outbox rows ────────────────────────────────────────────
    // kind='run' with payload that won't decode as RunEventEnvelope.
    sqlx::query(
        "INSERT INTO outbox (kind, run_id, payload) \
         VALUES ('run', 50000000001, '{\"bogus\":true}'::jsonb)",
    )
    .execute(&pool)
    .await
    .expect("bad run outbox insert failed");

    // kind='job' with payload that won't decode as JobEventEnvelope.
    sqlx::query(
        "INSERT INTO outbox (kind, run_id, payload) \
         VALUES ('job', 50000000001, '{\"bogus\":true}'::jsonb)",
    )
    .execute(&pool)
    .await
    .expect("bad job outbox insert failed");

    // ── Force a backstop rescan via manual NOTIFY ─────────────────────────────
    // NOTIFY '1' → listener fetch_min(1) → drain backstop=1 →
    // pass_start_floor = min(0, 0) = 0 → SELECT > 0 returns both bad rows.
    sqlx::query("SELECT pg_notify('atc_outbox', '1')")
        .execute(&pool)
        .await
        .expect("manual NOTIFY failed");

    // ── Wait for drain to complete the pass ──────────────────────────────────
    timeout(Duration::from_secs(10), async {
        loop {
            tokio::time::sleep(Duration::from_millis(30)).await;
            if fixture.observed_passes.load(Ordering::Relaxed) > passes_after_startup {
                return;
            }
        }
    })
    .await
    .expect("drain did not complete the pass within 10s");

    // ── Assert: no broadcast events emitted for bad rows ─────────────────────
    assert!(
        rx.try_recv().is_err(),
        "no SeqEvents should be broadcast for rows with bogus payloads"
    );

    // ── Assert metric counters ────────────────────────────────────────────────
    let after_rows = parse_unlabeled_counter(&render_metrics(), "atc_pg_drain_rows_total");
    let after_dup =
        parse_unlabeled_counter(&render_metrics(), "atc_pg_drain_duplicate_skipped_total");
    let after_unknown =
        parse_unlabeled_counter(&render_metrics(), "atc_pg_drain_unknown_kind_total");

    assert_eq!(
        after_rows - baseline_rows,
        2,
        "drain should have processed 2 bad rows; baseline={baseline_rows} after={after_rows}"
    );
    assert_eq!(
        after_dup, baseline_dup,
        "duplicate-skip counter must not advance for bogus-payload rows"
    );
    assert_eq!(
        after_unknown, baseline_unknown,
        "unknown-kind counter must not advance (no unknown-kind rows inserted; \
         the CHECK constraint prevents kind outside 'run'/'job')"
    );

    fixture.shutdown.cancel();
}
