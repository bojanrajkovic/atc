//! Span-side instrumentation tests for the in-memory eviction task.
//!
//! Each `evict_expired` call emits its own `eviction.sweep` root span carrying
//! `jobs.evicted`, `runs.evicted`, and `elapsed.micros` fields. Per-tick roots
//! are the shared convention across the long-lived background tasks
//! (`eviction.sweep`, `listener.recv`, `drain.pass`): a task-lifetime parent
//! would never end until process shutdown, so each tick would attach to a span
//! the SDK couldn't export until then. Per-tick roots mean every sweep is one
//! tidy trace that exports on tick.

use std::sync::Arc;
use std::time::Duration;

use atc_core::clock::TestClock;
use atc_core::event::{JobEvent, JobEventEnvelope, RunEvent, RunEventEnvelope};
use atc_core::types::{JobId, RunId};
use atc_core::{JobConclusion, fixed_test_timestamp};
use atc_persist::PersistentStore;
use atc_store_mem::InMemoryStore;
use chrono::TimeDelta;
use opentelemetry_sdk::trace::SpanData;
use serial_test::serial;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::common;

fn run_requested(run_id: i64) -> RunEventEnvelope {
    common::make_run_envelope(RunId(run_id), RunEvent::Requested)
}

fn job_completed(job_id: i64, run_id: i64) -> JobEventEnvelope {
    let now = fixed_test_timestamp();
    JobEventEnvelope {
        job_id: JobId(job_id),
        run_id: RunId(run_id),
        org: "test-org".to_string(),
        repo: "test-repo".to_string(),
        name: "Eviction test job".to_string(),
        created_at: now,
        started_at: Some(now),
        completed_at: Some(now),
        action: JobEvent::Completed {
            conclusion: JobConclusion::Success,
            runner: None,
            labels: vec![],
            steps: vec![],
        },
    }
}

fn spans_named<'a>(spans: &'a [SpanData], name: &str) -> Vec<&'a SpanData> {
    spans.iter().filter(|s| s.name.as_ref() == name).collect()
}

/// OpenTelemetry's `Value` enum has no unsigned-integer variant — the
/// `tracing-opentelemetry` bridge converts `u64`/`u128` field values to
/// `String` rather than truncating to `i64`. Parse both representations so
/// span fields are recoverable regardless of which numeric type the
/// production code recorded.
fn attribute_u64(span: &SpanData, key: &str) -> Option<u64> {
    use opentelemetry::Value;
    span.attributes.iter().find_map(|kv| {
        if kv.key.as_str() != key {
            return None;
        }
        match &kv.value {
            Value::I64(v) => u64::try_from(*v).ok(),
            Value::String(s) => s.as_str().parse::<u64>().ok(),
            _ => None,
        }
    })
}

#[tokio::test]
#[serial]
async fn eviction_sweep_emits_span_with_counts_and_elapsed() {
    common::ensure_recorder_installed();
    common::reset_spans();

    // Seat a completed job past the configured TTL on a TestClock. The store's
    // `eviction.sweep` `#[tracing::instrument]` fires on each direct call to
    // `evict_expired`, so we exercise it directly here for deterministic field
    // assertions independent of ticker cadence.
    let start_time = fixed_test_timestamp();
    let clock = Arc::new(TestClock::new(start_time));
    let ttl = Duration::from_secs(60 * 60);
    let store = InMemoryStore::new_for_test(clock.clone(), ttl, 256);

    let run_id: i64 = 9_300_001;
    let job_id: i64 = 9_300_002;
    store
        .apply_run_event(run_requested(run_id))
        .await
        .expect("apply_run_event");
    store
        .apply_job_event(job_completed(job_id, run_id))
        .await
        .expect("apply_job_event");

    // Advance past TTL so the next sweep evicts the job and (because it was the
    // only job for this run) the parent run as well.
    clock.advance(TimeDelta::hours(2));
    store.evict_expired().await;

    let spans = common::read_finished_spans();
    // The InMemorySpanExporter is process-global and the consolidated
    // integration binary serializes span-reading tests via #[serial], but
    // the buffer is never reset between tests in a `#[serial]` group — only
    // `reset_spans()` at this test's entry clears prior output. Filter to
    // the sweep that matches THIS seed (1 job evicted) rather than taking
    // the first `eviction.sweep` we see, so a stray no-op sweep from any
    // other test cannot leak into the assertion.
    let sweep = spans_named(&spans, "eviction.sweep")
        .into_iter()
        .find(|s| attribute_u64(s, "jobs.evicted") == Some(1))
        .expect("eviction.sweep span with jobs.evicted=1 must be exported after evict_expired");

    assert_eq!(
        attribute_u64(sweep, "runs.evicted"),
        Some(1),
        "eviction.sweep must record runs.evicted=1 (orphaned run reaped); attributes={:?}",
        sweep.attributes,
    );
    assert!(
        attribute_u64(sweep, "elapsed.micros").is_some(),
        "eviction.sweep must record elapsed.micros (non-negative u64); attributes={:?}",
        sweep.attributes,
    );
}

#[tokio::test]
#[serial]
async fn spawned_sweep_emits_root_span_with_no_parent() {
    common::ensure_recorder_installed();
    common::reset_spans();

    // Use `InMemoryStore::start` so the eviction task is actually spawned.
    // A short eviction_period lets the ticker fire at least once before the
    // test cancels. The key assertion is that the sweep span has NO parent:
    // there is no task-lifetime `eviction.task` wrapper, so each tick is its
    // own root that exports on tick (rather than accumulating under a parent
    // span that only ends at process shutdown).
    let shutdown = CancellationToken::new();
    let clock = Arc::new(TestClock::new(fixed_test_timestamp()));
    let store = InMemoryStore::start(
        clock.clone(),
        Duration::from_secs(60 * 60),
        Duration::from_millis(40),
        shutdown.clone(),
    );

    // Seat an evictable job and advance the TestClock past TTL so the spawned
    // sweep has real work to record into the span fields.
    let run_id: i64 = 9_300_101;
    let job_id: i64 = 9_300_102;
    store
        .apply_run_event(run_requested(run_id))
        .await
        .expect("apply_run_event");
    store
        .apply_job_event(job_completed(job_id, run_id))
        .await
        .expect("apply_job_event");
    clock.advance(TimeDelta::hours(2));

    // Poll for a sweep span where `jobs.evicted=1`. The first ticker firing
    // happens between `interval` and `2*interval` after spawn, and tests in
    // the shared binary may have left earlier no-op sweep spans in the
    // exporter from before our `reset_spans()` reset; filter to the sweep
    // that actually evicted to avoid relying on tick ordering.
    let mut active_sweep: Option<SpanData> = None;
    for _ in 0..50 {
        let spans = common::read_finished_spans();
        if let Some(s) = spans_named(&spans, "eviction.sweep")
            .into_iter()
            .find(|s| attribute_u64(s, "jobs.evicted") == Some(1))
        {
            active_sweep = Some(s.clone());
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let sweep = active_sweep.expect(
        "eviction.sweep span with jobs.evicted=1 must be exported after the spawned ticker fires",
    );

    assert!(
        sweep.parent_span_id.to_bytes().iter().all(|b| *b == 0),
        "eviction.sweep emitted from the spawned task must be a root span \
         (zero parent_span_id). A non-zero parent here means a task-lifetime \
         wrapper was reintroduced — see the per-tick-root rationale in this \
         file's module doc."
    );

    shutdown.cancel();
    timeout(Duration::from_secs(5), store.shutdown())
        .await
        .expect("eviction task did not join within 5s");
}
