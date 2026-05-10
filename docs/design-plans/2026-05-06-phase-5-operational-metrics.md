# Phase 5 — Operational Metrics for the Postgres Drain Path (+ Grafana Template)

> **Implementation Guidance:** Before writing any code for this plan, read [`docs/implementation-guidance.md`](../implementation-guidance.md). That document — not this plan — governs how implementation is executed (TDD discipline, branch/PR conventions, doc-mapping updates, ADR annotation sweeps, generated-file rules).

**PR title (squash-merged commit subject):** `feat(server): add operational metrics for the postgres drain path`

## Context

PR #57 (commit `6b7d0e4`, Phase 4) closed issue #7 by gating multi-replica on a Postgres URL and removing the SQLite + persistence chart machinery. The runtime supports symmetric replicas, transactional outbox writes, LISTEN/NOTIFY drain, REPEATABLE READ snapshots cited against a per-replica `broadcast_watermark`, ring-buffer dedup (cap 2048), and a gap-healing backstop via `min_pending_seq`. The drain task is the sole writer to the broadcast channel in PG mode; `/readyz` 503s when the drain heartbeat is stale (30s threshold).

The system is correct. It is not currently observable. The `/metrics` endpoint exposes nine `atc_pg_*` counters (write failures, drift, NOTIFY emit/recv, listener errors, drain passes/rows, duplicate skipped, unknown kind — verified against `metrics.rs`) plus the `atc_build_info` gauge — counters that confirm "things happened" but say nothing about *latency*, *backlog*, or *concurrency pressure*. Operators running multi-replica today have no signal for "is this replica falling behind?" or "is gap-healing firing more than expected?" beyond inferring from rate-of-counters.

ADR 0002:212–213 explicitly defers operational metrics to Phase 5 ("outbox lag, forwarder watermark, wake-up coalescing, replay duration"). This plan ships those metrics, plus the per-replica `broadcast_watermark` and `min_pending_seq` cursors as gauges, the per-metric documentation operators need to interpret them, and a Grafana template dashboard as the consumption surface. Phase 5 is the last named phase in `state-externalization-research/rollout-and-implementation.md`; the remaining Phase 5 work (outbox retention, in-memory mode removal, raw-webhook persistence) is explicitly deferred to separate plans (see Out of Scope).

## Definition of Done

1. Six new metrics exposed at `/metrics`: `atc_pg_outbox_lag_seconds` (histogram), `atc_pg_drain_pass_duration_seconds` (histogram), `atc_pg_wake_coalesced_total` (counter), `atc_pg_drain_startup_seconds` (histogram), `atc_pg_broadcast_watermark` (gauge), `atc_pg_min_pending_seq` (gauge).
2. `outbox` table schema unchanged; the drain SELECT is extended to include the existing `inserted_at TIMESTAMPTZ` column so lag can be computed Rust-side.
3. Per-metric documentation in `docs/architecture/backend-server.md` covering: name, type, labels (and source — emitted vs scrape-injected), what it measures, per-replica vs cluster scope, aggregation guidance (avg/max/sum across replicas), example PromQL.
3a. A **durable "Metric authoring contract" subsection** added to `docs/architecture/backend-server.md` codifying the interpretation-surface requirement as a project-wide convention. The contract states: every metric added to this codebase MUST ship with the same seven-element block (name, type, labels with source, semantics, per-replica vs cluster scope, aggregation guidance, example PromQL) — not as a Phase 5 deliverable but as a forward-binding rule that every future plan inherits.
4. Backend integration tests assert each metric's behavior under a controlled scenario (one test file per metric, paired success/failure cases per the AC table). Tests primarily use the existing `render_metrics()` helper (renders the global recorder's exposition text in-process) for delta assertions; tests that explicitly need HTTP-side-port coverage scrape `/metrics` against the spawned TestApp's metrics listener. All tests use `#[serial_test::serial]` for global-recorder safety. No source-grep tests (per `feedback_no_source_grep_tests.md`).
5. Grafana template dashboard at `deploy/grafana/atc-postgres-overview.json` with panels covering all six metrics (drain throughput, outbox lag, drain-pass duration heatmap, wake-coalesced rate, drain startup duration distribution, watermark vs min_pending_seq). JSON parses cleanly; manually validated against a real PG-mode replica.
6. ADR 0002 "Out of scope" → "Implementation Status" sweep: the operational-metrics bullet (line 212–213) moves to a new "Implementation Status" appendix with a Phase 5 cross-link; the remaining Phase 5 deferrals stay in "Out of scope" with their separate-plan markers.
7. `state-externalization-research/rollout-and-implementation.md` Phase 5 list narrowed: the metrics bullet marked Done with date stamp; the other three bullets stay open with a note about separate plans.
8. `backend/crates/atc-server/CLAUDE.md` metric inventory extended to list the six new metrics with their semantic notes.
9. CONTRIBUTING.md gains a "Metrics" section covering both naming convention (`atc_` prefix, `pg_` subsystem, `_total` for counters, `_seconds` for histograms, gauge units in description) AND the metric authoring contract, with the canonical home cross-linked at `docs/architecture/backend-server.md` § "Metric authoring contract".
10. Manual smoke verification: `/metrics` scrape from a single-replica `just dev` PG-mode run shows all six metrics with sensible values; loading the Grafana JSON renders all panels with data.

## Locked Decisions (carried from Phases 1–4 — not open for re-evaluation)

- **Metrics crate stack: `metrics` v0.24.3 + `axum_prometheus` v0.10.0.** Verified at `backend/crates/atc-server/Cargo.toml`; current `metrics.rs` registers metric *descriptions* via `metrics::describe_*!` and emits via `counter!` / `gauge!` / `histogram!` macros. Per project memory `feedback_dont_assume_dep_minimalism.md`: this stack is in place; do not propose alternatives.
- **Naming convention.** `atc_` project prefix; `pg_` subsystem prefix for Postgres-path metrics; `_total` suffix for monotonic counters; `_seconds` suffix for time-valued metrics regardless of metric type (counter/gauge/histogram) — Prometheus best practice (`process_start_time_seconds` is a gauge, `axum_http_requests_duration_seconds` is a histogram); gauges that aren't time-valued carry no unit suffix (units documented in description). Verified against existing inventory at `metrics.rs:47,72,77,95,100,104,108,112,116,120` (line numbers in `metrics.rs` per the actual file structure read during plan authorship).
- **No metric carries a per-replica label.** Replica identity is added by the monitoring stack at scrape time as standard target labels (`pod`, `instance`). The exact mechanism depends on the deployment — Prometheus Operator's ServiceMonitor flow attaches these via service-and-pod target relabeling; non-Operator setups (VictoriaMetrics, Promtail-style scrapers, plain Prometheus with `kubernetes_sd_configs`) attach them via their own discovery rules. The chart's `deploy/helm/atc/templates/servicemonitor.yaml` has no `metricRelabelings` that would strip these. Per-replica scoping is implicit in *every* `atc_pg_*` metric — each pod emits its own value; queries aggregate via `avg by (pod)`, `max by (pod)`, etc.
- **Per-replica `broadcast_watermark`, drain is sole writer in PG mode.** Per ADR 0002 Decision 5; verified at `backend/crates/atc-server/src/main.rs:151` (per-process `Arc<AtomicI64>` init), `listener.rs:214` (Release store after successful drain pass), `routes.rs:113–115` (Acquire load before tx open), `listener.rs:344` (sole `webhook_tx.send` site in PG branch).
- **`outbox.inserted_at TIMESTAMPTZ DEFAULT now()` exists.** Verified at `backend/crates/atc-server/migrations/0002_outbox.sql:7`. The drain query (`listener.rs:273–289`) does NOT currently SELECT this column; Phase 5 adds it.
- **Tests use `render_metrics()` delta pattern with `#[serial_test::serial]`.** Verified at `backend/crates/atc-server/tests/common/mod.rs:34–40`. The Prometheus global recorder is installed once via `OnceLock`; tests assert deltas, not absolutes, because metric state persists across the suite.
- **`/readyz` drain heartbeat staleness threshold = 30s.** No change. Phase 5's metrics complement readiness; they do not gate it.
- **Listener URL plumbing supports pgbouncer split.** No change.
- **PG-side TTL eviction deferred to a separate Phase 5 sub-plan.** Per ADR 0003 Decision 4 — out of scope for the metrics chunk.
- **Symmetric replicas, no leader.** No change to the runtime topology.

## Architecture

### D1 — Metric shapes (six new metrics, no replica labels)

Each metric is registered in `metrics.rs` via `metrics::describe_*!` (description + unit hint where applicable) and emitted at the call sites in `listener.rs` / `main.rs`. The `atc_pg_*` prefix and unit-suffix convention are continued from existing metrics.

| Name | Type | Labels | Emitted from | What it measures |
|------|------|--------|--------------|------------------|
| `atc_pg_outbox_lag_seconds` | histogram | none | drain pass, post-broadcast (one observation per broadcast row) | Wall time `Utc::now() - row.inserted_at` recorded for every broadcast outbox row. Histogram captures distribution; alerts use `histogram_quantile(0.99, ...)`. |
| `atc_pg_drain_pass_duration_seconds` | histogram | none | drain pass | Wall time from drain-pass start to drain-pass exit, including all paginated batches in that pass. |
| `atc_pg_wake_coalesced_total` | counter | none | listener loop | Incremented when a NOTIFY arrives while `drain_in_flight=true` (a coalesced wake-up). |
| `atc_pg_drain_startup_seconds` | histogram | none | drain task, once | Startup initialization latency: wall time from `COALESCE(MAX(seq),0)` watermark init through first drain pass exit. One observation per process lifetime. **Not a "replay" metric**: per Phase 3c restart-recovery contract (`tests/phase_3c_restart_recovery.rs`), the new drain task initializes its watermark from `MAX(seq)` and does NOT replay historical outbox rows. The metric measures startup readiness latency, not catch-up backlog. |
| `atc_pg_broadcast_watermark` | gauge | none | drain pass, on watermark store | Mirrors the per-replica `broadcast_watermark: Arc<AtomicI64>` after each successful drain pass. |
| `atc_pg_min_pending_seq` | gauge | none | listener (on `fetch_min`) + drain (on swap) | Mirrors the per-replica `min_pending_seq: Arc<AtomicI64>` when it holds a real registered seq (gap-healing pressure visible). Set to `f64::NAN` when the underlying atomic is at its `i64::MAX` sentinel (no pending below-watermark NOTIFY) — see D6 for rationale. |

**Rejected: per-replica label baked into the metric.** Adding a `replica="atc-0"|"atc-1"` label would require Phase 5 to plumb a process identifier (downward API → env var → metric label) and would create a high-cardinality dimension that's already provided by Prometheus's standard `pod` label injected at scrape. The existing nine `atc_pg_*` counters do not carry a replica label; consistency wins. Documented in §"Per-metric documentation" so operators know to query with `by (pod)`.

**Why histogram (not gauge) for outbox lag.** A gauge would track "lag of the most recent broadcast" — but at 100+ broadcasts/sec the gauge changes faster than the 30s scrape interval, so what Prometheus captures is essentially a random sample within each scrape window. A histogram captures every observation; queries can compute p50/p99 distribution over time and alerts can use `histogram_quantile(0.99, sum(rate(atc_pg_outbox_lag_seconds_bucket[5m])) by (le, pod))`. Use the default histogram bucket distribution (`[0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10]`) — covers sub-millisecond healthy drain through 10s pathology cleanly without custom matcher configuration.

### D2 — Outbox lag computation (Rust-side, one observation per broadcast)

Add `inserted_at` to the drain query SELECT:
```sql
-- listener.rs:273-289 (current): SELECT seq, kind, payload FROM outbox WHERE seq > $1 ORDER BY seq LIMIT $2
-- Phase 5: SELECT seq, kind, payload, inserted_at FROM outbox WHERE seq > $1 ORDER BY seq LIMIT $2
```

At the broadcast site (`listener.rs:344`, after successful `webhook_tx.send`), record one histogram observation per broadcast row:
```rust
let lag = (Utc::now() - row.inserted_at).num_microseconds().unwrap_or(0) as f64 / 1_000_000.0;
metrics::histogram!("atc_pg_outbox_lag_seconds").record(lag);
```

**Systematic offset (documented for operators):** `inserted_at DEFAULT now()` evaluates `now()` = `transaction_timestamp()` = transaction *start*, not commit. The webhook handler's INSERT happens early in the transaction; commit happens later. Lag is therefore over-reported by the transaction duration (typically 5–50ms for a webhook), which is in the noise floor of a metric whose meaningful range is hundreds of milliseconds to seconds. No schema change to fix this; the per-metric doc in `backend-server.md` calls it out so operators don't chase phantom 50ms baseline lag.

**Rejected: PG-side server timestamp diff.** `EXTRACT(EPOCH FROM (clock_timestamp() - inserted_at))` returned as a query column would avoid clock-skew between PG server and the ATC pod (NTP-bounded, ~1–100ms). Round-trip cost is one column per row in a query that already returns 1–512 rows per pass; not worth the SQL/test complexity for a metric where the noise floor is already higher than the skew.

### D3 — Wake-coalesce instrumentation (shared `Arc<AtomicBool> drain_in_flight`)

Introduce `let drain_in_flight = Arc::new(AtomicBool::new(false));` in `main.rs` alongside the existing `broadcast_watermark` and `min_pending_seq` Arcs. Pass clones to both the listener task (`listener.rs:81-104`-area NOTIFY recv loop) and the drain task (`listener.rs:184-241` pass-execution block).

**Listener side** (NOTIFY recv): in `spawn_listener_task` (`listener.rs:76-109`), between the `min_pending_seq.fetch_min(...)` at line 88 and the `drain_notify.notify_one()` at line 98, check `drain_in_flight.load(Ordering::Acquire)`. If true, increment `atc_pg_wake_coalesced_total`. Then call `notify_one()` regardless (Tokio `Notify` handles the actual permit collapsing — the counter is observation-only).

**Drain side** (pass execution): in `spawn_drain_task` (`listener.rs:146-244`), bracket the `drain_pass(...).await` call at line 184–193 with two stores:
- `drain_in_flight.store(true, Ordering::Release)` immediately before the call (between the `pass_start_floor` calculation at line 182 and the call at line 184).
- `drain_in_flight.store(false, Ordering::Release)` immediately after the call returns (before the `atc_pg_drain_passes_total` increment at line 195).

This bracket is two unconditional lines, no scope guard or panic-safety needed: `drain_pass` is `async` but cannot panic-unwind across the await boundary in Tokio (panics terminate the task; the AtomicBool would simply stay `true` for a dead task — operationally identical to "the drain task is gone, NOTIFYs accumulate as permits"). No new direct dependency required.

**Definition the metric counts:** "NOTIFY arrivals observed by the listener while `drain_in_flight` is true." This matches the user's wording exactly. Whether the drain task does an *extra* pass for each coalesced NOTIFY is a separate question (Tokio's `Notify` collapses N permits into 1) — the metric is about NOTIFY arrival rate vs drain pass rate, which is what operators actually want.

**Rejected: bounded channel capacity 1.** A `mpsc::channel(1)` between listener and drain would let the listener `try_send` and observe full-channel as the coalesce signal. Cleaner abstraction, but requires retiring the `Arc<Notify>` plumbing established in Phase 2d/3c. The `AtomicBool` overlay is smaller blast radius.

**Rejected: drain self-tracking via consecutive-permit count.** The drain's `notify.notified().await` consumes one permit at a time; we'd lose visibility into NOTIFYs collapsed during the pass.

### D4 — Drain startup timing (timer captured in main.rs, recorded in drain task)

The metric measures wall time from `COALESCE(MAX(seq),0)` watermark init through first drain pass exit. Spans two execution contexts: the watermark init runs in `main.rs` BEFORE `spawn_drain_task` is called (the COALESCE query lives at `main.rs:195`; the result is passed in as `initial_watermark: i64` per `listener.rs:135`); the first drain pass runs inside the spawned closure.

**Why not "replay duration":** the metric was originally framed as "replay" but Phase 3c established the contract that restart recovery does NOT replay historical events (`tests/phase_3c_restart_recovery.rs:T10` asserts a fresh drain task does NOT rebroadcast pre-restart events; watermark seeds from `MAX(seq)`, the next drain pass finds `seq > MAX(seq)` = empty, no historical rows broadcast). What the metric actually measures is "how long after process start can this replica serve traffic with a hot drain loop" — startup readiness. Renamed accordingly.

Implementation: `main.rs` captures `let startup_at = Instant::now();` immediately before the `COALESCE(MAX(seq),0)` query at `main.rs:195`. The Instant is threaded into `spawn_drain_task` as a new parameter (e.g., alongside `initial_watermark`). Inside the drain task closure, a `let mut startup_recorded = false;` flag tracks whether the first pass has been observed; after the first `drain_pass(...).await` returns (success OR failure — startup duration measures wall time, not success), if `!startup_recorded`, observe `metrics::histogram!("atc_pg_drain_startup_seconds").record(startup_at.elapsed().as_secs_f64())` and set the flag.

Threading the Instant through one parameter is cheaper than externalizing the timer via a one-shot Notify (the rejected alternative). Encapsulating the observation site inside the drain task means the metric is emitted exactly once per process even if main.rs's lifecycle changes later.

**Rejected: drain task self-times from spawn entry.** Excludes the COALESCE query cost. The COALESCE round-trip can be the dominant cost on a cold connection pool.

**Rejected: main.rs externally times via Notify.** Adds one-shot Notify plumbing for a metric with one observation per process. Threading an Instant is one fewer moving part.

### D5 — Histogram bucket configuration (custom for drain-startup, defaults for drain-pass)

`metrics-exporter-prometheus` uses `[0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10]` as default histogram buckets when no custom matcher fires.

- **`atc_pg_drain_pass_duration_seconds`**: default buckets. Typical pass duration is 1–50ms (single-row, no batching) or 50–500ms (large pagination batch). Defaults cover this range with reasonable resolution; no observed need to customize.
- **`atc_pg_drain_startup_seconds`**: custom buckets `[0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]`. Per Phase 3c there's no historical-replay backlog; typical startup latency is dominated by COALESCE round-trip (~5–50ms on a warm connection, possibly 500ms+ on a cold pool) plus drain task scheduling. Custom buckets are tighter at the low end and stop at 10s — values >10s indicate a real startup pathology (DB unreachable, lock contention) that warrants alerting, not finer bucket resolution.

Configure via `PrometheusBuilder::set_buckets_for_metric(Matcher::Full("atc_pg_drain_startup_seconds".to_string()), &[0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0])` — see D5b for the integration path that preserves existing axum-prometheus HTTP buckets.

### D5b — Custom recorder integration (preserves existing HTTP duration buckets, includes upkeep)

The current `metrics.rs:28` calls `PrometheusMetricLayer::pair()`, which (per axum-prometheus 0.10.0 source) internally calls `PrometheusBuilder::build_recorder()`, registers the recorder as global via `metrics::set_global_recorder`, and spawns a 5-second upkeep loop calling `handle.run_upkeep()`. To install custom histogram buckets we cannot use `pair()` (no override hook for buckets) and we must not double-install (panics with duplicate-recorder error per the file's existing doc comment).

The supported pattern: install our own `PrometheusBuilder` (which becomes the global recorder for `metrics::*` macros), spawn the upkeep loop ourselves, then construct the axum-prometheus layer:

```rust
use std::time::Duration;
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use axum_prometheus::PrometheusMetricLayer;

let handle: PrometheusHandle = PrometheusBuilder::new()
    .set_buckets_for_metric(
        Matcher::Full("atc_pg_drain_startup_seconds".to_string()),
        &[0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0],
    )
    .expect("valid bucket spec")
    .install_recorder()
    .expect("install recorder");

// Replicate axum-prometheus's upkeep behavior: handle.run_upkeep() collapses
// per-thread storage on a 5s cadence. Without this, summary/histogram data
// staleness diverges from the existing pair() behavior. axum-prometheus 0.10.0
// pair() spawns this loop internally; when we hand-install the recorder, we
// must spawn it ourselves.
let upkeep_handle = handle.clone();
tokio::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    loop {
        interval.tick().await;
        upkeep_handle.run_upkeep();
    }
});

// PrometheusMetricLayer::new() does not install a recorder; it records to
// whichever recorder is global. Our installed one captures axum-prometheus's
// emissions (axum_http_requests_*) AND our atc_* emissions through the same
// handle.
let layer = PrometheusMetricLayer::new();
```

**Existing HTTP duration buckets are preserved** in the sense that `axum_http_requests_duration_seconds` (which the layer emits via the `metrics::histogram!` macro) records into our globally-installed recorder. Histograms that don't match a `set_buckets_for_metric` matcher fall through to `metrics-exporter-prometheus`'s default bucket distribution (the standard `[0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10]`), so `axum_http_requests_duration_seconds_bucket{le="..."}` lines still appear in `/metrics` output. Existing test assertions in `tests/metrics.rs:165-166` continue to pass. **Verify this against `metrics-exporter-prometheus` 0.18.x at the time of implementation** — if the default-bucket fallback behavior has changed, add an explicit `set_buckets_for_metric(Matcher::Suffix("_seconds".to_string()), &standard_buckets)` to make the default explicit.

**Test fixture must mirror this pattern.** `tests/common/mod.rs` currently calls `PrometheusMetricLayer::pair()` at the equivalent fixture-setup sites. Step 2 updates the fixture to use the same install-recorder-plus-upkeep pattern as production; otherwise tests would validate the old default-recorder path while production uses the new path, masking a real divergence (per memory `feedback_verify_invariant_layer.md` — verify the invariant lives in the runtime path the tests traverse).

**Doc-comment update required:** the comment at `metrics.rs:25-27` says "Do not call `PrometheusBuilder::install()` separately — axum-prometheus installs the recorder internally." Phase 5 reverses this — we DO install it separately. Update the comment to: "PrometheusBuilder::install_recorder() is called explicitly to configure custom histogram buckets. The 5-second upkeep loop is spawned manually (axum-prometheus's pair() spawns it internally; when we hand-install we own this responsibility). PrometheusMetricLayer::new() does not install a recorder; it records to the global one we installed."

### D6 — Watermark gauge mirror sites

`atc_pg_broadcast_watermark`: at each `broadcast_watermark.store(new_seq, Ordering::Release)` site (currently `listener.rs:214`), also call `metrics::gauge!("atc_pg_broadcast_watermark").set(new_seq as f64)`. The store-site colocation guarantees the gauge is fresh on every scrape (max staleness = scrape interval = 30s per `servicemonitor.yaml`).

`atc_pg_min_pending_seq`: at each `min_pending_seq.fetch_min(seq, Ordering::Release)` site in the listener task (`listener.rs:88`), compute the new value as `prev.min(seq)` from the `fetch_min` return value (avoids a racing reload). If the new value is `i64::MAX` (no real registration happened — `seq >= prev` and prev was already MAX), set the gauge to `f64::NAN`; otherwise set it to `new_val as f64`. At the drain-task swap site (`listener.rs:181`, `AcqRel`-ordered swap to `i64::MAX`), set the gauge to `f64::NAN` after the swap — sentinel state ("no pending NOTIFY below watermark, drain has caught up").

**Why NaN instead of `i64::MAX as f64`:** `i64::MAX as f64` ≈ `9.22e18` exceeds float64's 53-bit integer precision and would render the watermark-vs-min_pending_seq Grafana panel useless (the y-axis would auto-scale to `~9e18`, hiding the actual divergence signal at the watermark level). Prometheus exposition supports NaN as a valid gauge value; clients render it as "no data" or filter it out via `... unless on() (atc_pg_min_pending_seq != atc_pg_min_pending_seq)`. Document the NaN sentinel in the per-metric block so operators understand why the gauge is intermittently absent from the panel.

### D7 — Metric authoring contract (durable convention) + per-metric documentation

The architecture doc gains TWO additions in distinct subsections:

**D7a — Metric authoring contract (forward-binding rule).** A new subsection at the top of `docs/architecture/backend-server.md` § Metrics, titled "Metric authoring contract." This codifies the interpretation-surface principle as a durable project convention:

> Every metric exposed at `/metrics` MUST ship with documentation in this section covering its interpretation surface — the contextual information an operator needs to read alerts, build dashboards, and decide which aggregator to use. Specifically, every metric documents:
>
> 1. **Name** — exact metric family name as scraped.
> 2. **Type** — counter / gauge / histogram.
> 3. **Labels** — every label name AND its source. Distinguish *emitted* labels (added by the application) from *scrape-injected* labels (e.g., `pod`, `instance`, added by the ServiceMonitor at scrape time).
> 4. **Measures** — one sentence stating what the metric value means in operational terms (not implementation terms).
> 5. **Per-replica vs cluster scope** — is the value a property of one replica's process state, or a cluster-wide invariant? This determines whether dashboards aggregate `by (pod)` or `without (pod)`.
> 6. **Aggregation guidance** — recommended cross-replica aggregator (`avg`/`max`/`sum`/`p99`) with one-sentence rationale.
> 7. **Example PromQL** — one canonical query that operators can copy-paste into Grafana to see meaningful data.
>
> This contract applies to every metric added to the codebase, not just Postgres-path metrics. Plans that add metrics MUST extend this section with the new metric's seven-element block before merge. The doc-staleness gate (`scripts/check-docs-lefthook.sh`) enforces that backend metric changes must update `backend-server.md`; this contract narrows the requirement from "update the doc" to "update the doc with the seven-element block."

The contract is stated once in this canonical home; CONTRIBUTING.md cross-links to it (Step 6) rather than duplicating.

**D7b — Operational metrics table (Phase 5 deliverable).** Phase 5 ships the six new metrics' interpretation-surface blocks immediately below the existing "LISTEN/NOTIFY metrics" table at `backend-server.md:251–263`. The new section is titled "Operational metrics" (no phase reference per memory `feedback_phases_not_in_user_facing_strings.md` — historical sections allow phase markers, this is forward-facing). For each of the six new metrics, the section has a per-metric subsection conforming to the seven-element block from D7a.

A short prose paragraph above the operational-metrics table explains the per-replica scoping convention specific to this codebase: "All `atc_pg_*` metrics are emitted unlabeled per-process. Replica identity is added by the monitoring stack at scrape time as standard target labels (`pod`, `instance`) — the exact attachment mechanism depends on the deployment (Prometheus Operator ServiceMonitor, plain Prometheus with `kubernetes_sd_configs`, VictoriaMetrics, etc.); the metrics themselves are agnostic. Cross-replica aggregation in alerts and dashboards uses `avg by (pod)`, `max by (pod)`, etc."

The seven existing Phase 2d/3c metrics (already in the LISTEN/NOTIFY table) ARE retroactively in scope of the contract, but Phase 5 does NOT backfill seven-element blocks for them — that's a follow-up doc-debt item explicitly noted in §Out of Scope below. The contract is forward-binding from this plan's merge date.

### D8 — Grafana template dashboard

New file: `deploy/grafana/atc-postgres-overview.json`. Six panels minimum, covering the six new metrics directly (panels 1–4 + 6 below) plus one panel that combines two of the new gauges (panel 5). Where existing metrics provide useful context (panel 6's `atc_pg_drain_passes_total`), they're incorporated as supplementary series.

1. **Outbox lag (p99)** — `histogram_quantile(0.99, sum(rate(atc_pg_outbox_lag_seconds_bucket[5m])) by (le, pod))`, threshold lines at 1s (warn), 5s (alert). Time-series per pod.
2. **Drain pass duration heatmap** — histogram, `histogram_quantile(0.99, sum(rate(atc_pg_drain_pass_duration_seconds_bucket[5m])) by (le, pod))`.
3. **Wake-coalesce rate** — `rate(atc_pg_wake_coalesced_total[5m]) by (pod)` — high values indicate NOTIFY storm or slow drain.
4. **Drain startup latency** — `histogram_quantile(0.99, sum(rate(atc_pg_drain_startup_seconds_bucket[1h])) by (le, pod))` across the last hour; rare event (one observation per process), panel will mostly be empty between deploys.
5. **Watermark vs min_pending_seq** — two-series line chart per pod, queries `atc_pg_broadcast_watermark` and `atc_pg_min_pending_seq`. The min_pending_seq series is intermittently NaN (sentinel state per D6); Grafana renders NaN as gaps in the line. Divergence (min_pending_seq < watermark) signals gap-healing rescan in flight.
6. **Drain throughput** — `rate(atc_pg_drain_rows_total[5m]) by (pod)` and `rate(atc_pg_drain_passes_total[5m]) by (pod)`. Uses existing Phase 2d/3c counters as supplementary context for interpreting the new metrics; not a "new metric panel" but operationally adjacent.

The dashboard JSON uses the `Prometheus` datasource by name (a top-level Grafana convention; users with a differently-named datasource will rename via Grafana's UI on import). Variables: `$pod` (multi-select, from `label_values(atc_pg_drain_passes_total, pod)`).

**Validation:** the dashboard parses as JSON (`jq -e . deploy/grafana/atc-postgres-overview.json > /dev/null`) and is manually loaded into a real Grafana instance against a PG-mode replica generating data. Both checks belong in the implementation context's verification steps; the JSON-parse check is a CI-able assertion (added as a step in the verification recipe), while the Grafana-load check is manual and recorded in the PR's first-comment test plan.

### D9 — No chart, CI workflow, or doc-mapping changes

Per the coupling-site investigation (verified read-only):
- `deploy/helm/atc/values.yaml`, `templates/servicemonitor.yaml`, `templates/deployment.yaml`, `deploy/helm/atc/CLAUDE.md` — all metric-agnostic; no change.
- `.github/workflows/ci.yml` — `cargo test --workspace` (`ci.yml:147`) picks up new tests automatically; no metric-name greps anywhere in the matrix.
- `scripts/doc-mapping.sh` — `backend/crates/atc-server/src/*` already maps to `docs/architecture/backend-server.md`; the doc-staleness gate (`scripts/check-docs-lefthook.sh`) blocks the push if metrics changes land without the matching doc update — this is the desired behavior.
- `justfile` — `just test`, `just lint`, `just helm-lint`, `just helm-unittest` recipes verified to exist; cite by name only after verifying (per memory `feedback_verify_just_recipes_before_citing.md`).

**Note on `deploy/grafana/`:** this is a new directory at repo root level. `scripts/doc-mapping.sh` does not need to gate on it (Grafana JSON has no canonical doc). The architecture doc references the dashboard's purpose; the JSON itself is the artifact.

## Implementation Phases

> Phase 5 follows TDD discipline per `docs/implementation-guidance.md` Rule 2. The ordering is: failing tests → implementation → docs → dashboard → verification. The implementation context should dispatch sub-agents (`project-claude-librarian`, `codebase-investigator`) for the doc-update steps, per the user's preserve-context guidance.

### Step 1 — Backend integration tests (write failing tests first)

Six new test files in `backend/crates/atc-server/tests/`, one per metric. All use `#[serial_test::serial]` and the existing `render_metrics()` helper from `tests/common/mod.rs:34-40`. Each test asserts a *delta* (post-scenario value minus pre-scenario value) against the expected family/value rendered by `render_metrics()`. Where an AC explicitly requires HTTP-side-port coverage, the test uses an actual HTTP `/metrics` scrape against the spawned `TestApp`'s metrics listener; this is called out per file. Per memory `feedback_no_source_grep_tests.md`, none of these tests grep source files for metric names.

**Important harness prerequisite:** `render_metrics()` panics if the global recorder isn't yet installed. Each test file MUST first ensure the recorder is installed (via the existing `PROMETHEUS_INIT` helper in `tests/common/mod.rs`, which Step 2 updates to mirror D5b's install-recorder + upkeep pattern). The simplest pattern is to construct any `TestApp` first (which routes through the recorder-init helper); all baseline scrapes happen AFTER that initialization.

- `metrics_outbox_lag_test.rs` — Initialize recorder via baseline TestApp construction. **AC1 (success):** baseline scrape via `render_metrics()`, POST a webhook, sleep 2 seconds (forces lag), wait for drain, post-scrape, assert `atc_pg_outbox_lag_seconds_count` delta is 1 and `_sum` delta is in `[0.0, 5.0]` seconds. **AC2 (failure):** as a separate test in the same file, baseline scrape, INSERT an outbox row directly via SQL with `inserted_at = NOW() + INTERVAL '10 minutes'`, drive the drain pass to broadcast it, post-scrape, assert `_count` delta is 1 (the histogram recorded an observation despite the unusual input — no panic, no input-side clamping). Do NOT assert on `_sum` in this case; exporter handling of negative histogram observations varies. The wait-for-drain primitive already exists in `tests/common/mod.rs` from Phase 2d/3c.
- `metrics_drain_pass_duration_test.rs` — Initialize, baseline scrape, POST a webhook, wait for drain, scrape. **AC3 (success):** assert `atc_pg_drain_pass_duration_seconds_count` increments by ≥ 1 and `_sum` increment is in `[0.0001, 1.0]`. **AC3b (failure, second test in same file):** construct an idle TestApp (no webhooks), wait `2 * HEARTBEAT_TICK` (= 10s) so heartbeat-only ticks fire, scrape, assert `_count` delta is 0. This validates that heartbeat-only wakes do NOT execute a drain pass per `listener.rs:168-176`.
- `metrics_wake_coalesced_test.rs` — Use the slow-drain test fixture from `notify_listener_tests.rs` (`drain_delay = 200ms` per Phase 2d AC7). **AC4 (success):** baseline scrape, POST 5 webhooks rapidly within the drain window, wait for drain queue to drain fully, scrape, assert `atc_pg_wake_coalesced_total` increment is `≤ 5` (no over-counting). The lower bound is intentionally 0 — see AC4 rationale. **AC5 (failure, second test in same file):** baseline scrape, POST 3 webhooks with explicit `wait_for_drain_pass_complete()` between each (no overlap with an in-flight pass), scrape, assert `atc_pg_wake_coalesced_total` delta is exactly 0. AC4 + AC5 together enforce the counter's correctness contract without requiring deterministic synchronization between listener and drain task scheduling.
- `metrics_drain_startup_test.rs` — `render_metrics()` panics if called before the global recorder is installed (`tests/common/mod.rs` `PROMETHEUS_INIT` `OnceLock` — initialized by the build helper). The test must therefore initialize the recorder first. Pattern: (1) Call the test fixture's recorder-init helper (whatever Step 2 names it after refactoring `tests/common/mod.rs` per D5b's test-fixture mirror) so `PROMETHEUS_INIT` is set. (2) Capture baseline `_count` via `render_metrics()`. (3) Construct a fresh `TestApp` (which spawns a fresh drain task and emits the startup observation). (4) Wait on the existing `drain_started: Arc<Notify>` first-pass signal. (5) Capture post `_count` via `render_metrics()`. Assert: `_count` delta equals 1 (the test's drain task added exactly one observation) and `_sum` delta is `> 0`. Per Phase 3c contract there's no historical-replay drain — the test just exercises "startup happened, was timed, was recorded once." Do NOT pre-seed the outbox; that would imply replay semantics this metric does not have.

  **AC6b extension within the same file:** drive a webhook through the same TestApp after step (5), wait for that pass to complete (drain_passes counter increments), and re-capture `_count`. Assert `_count` delta from step (5) is 0 — the once-only contract. AC6b is implemented by adding this trailing assertion to the same test, not by creating a separate file.
- `metrics_broadcast_watermark_test.rs` — POST 3 webhooks, wait for drain to advance through all 3, scrape. Assert `atc_pg_broadcast_watermark` gauge equals the highest seq committed (queryable via direct SQL: `SELECT MAX(seq) FROM outbox`).
- `metrics_min_pending_seq_test.rs` — Two paired tests in the same file:
  - **AC8 (success):** abort the drain task immediately after fixture init so the listener mirrors `min_pending_seq` into the gauge but no drain swap to NaN ever happens. Then `pg_notify('atc_outbox', '<seq>')` to drive a `fetch_min` registration. Scrape, assert `atc_pg_min_pending_seq` gauge is a finite numeric value below the current `atc_pg_broadcast_watermark` gauge. This avoids the microseconds-fragile race in the original "scrape during gap-healing" approach by removing the drain task from the runtime entirely.
  - **AC8b (steady-state failure):** standard fixture, fire N webhooks, wait for drain to fully catch up, scrape, assert `atc_pg_min_pending_seq` gauge is `NaN` (the sentinel — see D6).

After Step 1, all six tests should be RED (the metric registrations don't exist yet, and `render_metrics()` won't find them).

**Per project memory `feedback_no_source_grep_tests.md`:** these are behavioral tests that scrape `/metrics` and assert observed values. None of them grep source files for metric names. The grep-style assertions are reviewer concerns, not test concerns.

### Step 2 — Backend implementation (turn the new tests green)

This is the main-context implementation step. Sub-agent dispatch is not appropriate here — the changes span four files with shared invariants.

- `backend/crates/atc-server/src/main.rs`:
  - Add `let drain_in_flight = Arc::new(AtomicBool::new(false));` alongside existing `min_pending_seq` (`main.rs:149`) and `broadcast_watermark` (`main.rs:151`) Arcs.
  - Pass `drain_in_flight` clones to both `spawn_listener_task` (`main.rs:208`) and `spawn_drain_task` (`main.rs:215`) calls.
  - Capture `let startup_at = Instant::now();` immediately before the `COALESCE(MAX(seq),0)` query at `main.rs:195`; pass `startup_at: Instant` to `spawn_drain_task` as a new parameter.
  - After the watermark seed at `main.rs:205` (`broadcast_watermark.store(initial_watermark, Release)`), also emit `metrics::gauge!("atc_pg_broadcast_watermark").set(initial_watermark as f64)` to seed the gauge at boot.
- `backend/crates/atc-server/src/listener.rs`:
  - Update drain query SELECT in `drain_pass` (currently `listener.rs:273-289`) to `SELECT seq, kind, payload, inserted_at FROM outbox WHERE seq > $1 ORDER BY seq LIMIT $2`. The `sqlx::query!` macro will validate the new return shape against `.sqlx/` offline cache; **regenerate the cache as part of this step** via `cargo sqlx prepare --workspace` against a live PG (existing project pattern; CI requires the regenerated cache).
  - The drain query's row reader will gain `row.inserted_at: chrono::DateTime<chrono::Utc>` automatically from the macro return type (sqlx's `chrono` feature is already enabled — see `backend-server.md:169`).
  - Add a new parameter `startup_at: Instant` to `spawn_drain_task` (matches the `Instant` captured in main.rs per the main.rs change above). At the top of the spawned closure (around `listener.rs:147`), add `let mut startup_recorded = false;` alongside the existing `let mut watermark`, `recent_ring`, `recent_set`.
  - In the NOTIFY recv loop (`spawn_listener_task`, between `listener.rs:88` and `:98`): observe `drain_in_flight.load(Ordering::Acquire)` and increment `atc_pg_wake_coalesced_total` if true. After `min_pending_seq.fetch_min(seq, Release)` at line 88, compute `let new_min = prev.min(seq)` from the fetch_min return value and set the gauge: `metrics::gauge!("atc_pg_min_pending_seq").set(if new_min == i64::MAX { f64::NAN } else { new_min as f64 });`.
  - At drain-pass start (immediately before the `drain_pass(...).await` call at `listener.rs:184`): `drain_in_flight.store(true, Ordering::Release); let pass_start = Instant::now();`.
  - At drain-pass return (immediately after the `await` at `listener.rs:193`, before the `atc_pg_drain_passes_total` increment at line 195): `drain_in_flight.store(false, Ordering::Release); metrics::histogram!("atc_pg_drain_pass_duration_seconds").record(pass_start.elapsed().as_secs_f64()); if !startup_recorded { metrics::histogram!("atc_pg_drain_startup_seconds").record(startup_at.elapsed().as_secs_f64()); startup_recorded = true; }`.
  - At the broadcast site inside `drain_pass` (after `webhook_tx.send` at `listener.rs:344-347`): `let lag = (chrono::Utc::now() - row.inserted_at).num_microseconds().unwrap_or(0) as f64 / 1_000_000.0; metrics::histogram!("atc_pg_outbox_lag_seconds").record(lag);` — one observation per broadcast row.
  - At watermark store site (`broadcast_watermark.store(...)` at `listener.rs:214`): also emit `metrics::gauge!("atc_pg_broadcast_watermark").set(watermark as f64);`. At drain-task spawn after the `COALESCE(MAX(seq),0)` initialization, also emit the gauge to seed the value at boot.
  - At drain-side swap of `min_pending_seq` (`listener.rs:181`): after the swap, emit `metrics::gauge!("atc_pg_min_pending_seq").set(f64::NAN)` (sentinel — see D6).
- `backend/crates/atc-server/src/metrics.rs`:
  - Six new `metrics::describe_*!` calls (per D1 table). Descriptions are operator-visible at scrape time — keep them implementation-detail-free; do NOT include "Phase 5" or "ADR 0002" or any internal-history strings (per `feedback_phases_not_in_user_facing_strings.md`).
  - Refactor `build()` (currently at `metrics.rs:28`) to install a custom `PrometheusBuilder` first, then construct `PrometheusMetricLayer::new()` recording to the now-installed global recorder. Exact pattern in D5b. This change preserves the existing default histogram buckets used by `axum_http_requests_duration_seconds` (which existing tests at `tests/metrics.rs:165-166` assert), while adding the custom bucket overlay for `atc_pg_drain_startup_seconds` only.
  - Update the doc-comment at `metrics.rs:25-27` per D5b.
- `backend/crates/atc-server/src/state.rs`: no changes (the gauge mirrors live where the underlying `Arc<AtomicI64>` is mutated, not where it's defined).
- `backend/crates/atc-server/src/routes.rs`: no changes.
- `backend/crates/atc-server/tests/common/mod.rs`: update both `PrometheusMetricLayer::pair()` call sites (around lines 56 and 219 in the current file — verify before editing) to mirror D5b's production pattern: install a custom `PrometheusBuilder` (with `set_buckets_for_metric` for `atc_pg_drain_startup_seconds`), spawn the upkeep loop, and construct `PrometheusMetricLayer::new()`. Without this, the test fixtures use the old default-recorder path while production uses the new install-recorder path — bugs in D5b would not be caught by tests (per memory `feedback_verify_invariant_layer.md`).

After Step 2, run `just test`. All six new tests should be GREEN; existing 255 backend tests should remain GREEN (no regressions).

### Step 3 — Architecture-doc contract + per-metric docs in `backend-server.md` (sub-agent dispatch)

Dispatch a `project-claude-librarian` sub-agent with the exact change spec:

**3a — Metric authoring contract (durable convention).** Add a new subsection titled "Metric authoring contract" at the top of `backend-server.md` § Metrics — specifically, after line 186 (the `## Metrics` header) and before line 194 (the `### axum-prometheus placement` subsection). The contract sits at the top of § Metrics so it scopes every later subsection in that section. The subsection contains the literal blockquote from D7a above, including the seven-element list and the doc-staleness-gate cross-reference. The sub-agent's prompt MUST pass the literal blockquote text — no paraphrasing — so the contract reads identically to its definition in this plan.

**3b — Per-replica scoping prose paragraph.** Add the prose paragraph from D7b ("All `atc_pg_*` metrics are emitted unlabeled per-process...") immediately above the operational-metrics subsection.

**3c — Operational metrics subsection (six per-metric blocks).** Add an "Operational metrics" subsection immediately below the existing "LISTEN/NOTIFY metrics" table (currently `backend-server.md:251–263`). For each of the six new metrics (`atc_pg_outbox_lag_seconds`, `atc_pg_drain_pass_duration_seconds`, `atc_pg_wake_coalesced_total`, `atc_pg_drain_startup_seconds`, `atc_pg_broadcast_watermark`, `atc_pg_min_pending_seq`), add a per-metric subsection with the seven-element block: Name, Type, Labels (with `(scrape-injected)` annotation for pod/instance), Measures (one sentence), Per-replica (Yes for all six, with a one-sentence rationale), Aggregation (recommended cross-replica aggregator + rationale), Example PromQL.

**3d — Startup-behavior table row.** Add a row to the startup-behavior table at `backend-server.md:149-157`: "PG mode startup → first drain pass complete: latency observed via `atc_pg_drain_startup_seconds` (one observation per process lifetime)."

**3e — `Last verified:` stamp.** Update the file's stamp with the implementation landing date (do not pre-fill from this plan).

The sub-agent prompt MUST include canonical file paths and the literal content blocks to add (per memory `feedback_dont_assume_dep_minimalism.md`'s sister concept: subagents fabricate plausible-but-wrong content if given pattern descriptions instead of literal strings). Pass each per-metric block as a literal markdown subsection, not a "fill in this template" instruction.

### Step 4 — Grafana dashboard JSON

Create `deploy/grafana/atc-postgres-overview.json` from a Grafana 11.x export template. The JSON must:
- Define six panels with the queries and thresholds from D8.
- Use a single `$pod` template variable backed by `label_values(atc_pg_drain_passes_total, pod)`.
- Reference the datasource by name `Prometheus` (canonical convention; user adjusts on import if their datasource is named differently).
- Parse cleanly with `jq -e .`.

This file is hand-authored from a small starter template (Grafana 11.x dashboard JSON has stable structure; we don't need to export from a live Grafana to author it). The implementation context can use a sub-agent to compose the JSON if it has Grafana experience, or hand-author panel-by-panel.

**Validation:**
- `jq -e . deploy/grafana/atc-postgres-overview.json > /dev/null` — JSON parses.
- Manual: load the JSON into a real Grafana instance pointed at a `just dev` PG-mode replica generating data; confirm all six panels render. This step is the closure evidence; capture screenshots in the PR's first-comment test plan.

### Step 5 — Architecture and ADR doc updates (sub-agent dispatch)

Dispatch a `project-claude-librarian` sub-agent with the exact change spec for these files. Pass canonical URLs/IDs and literal-string content per the planning-workflow conventions.

- `docs/architecture-decisions/0002-state-externalization-postgres-outbox.md`:
  - Move the operational-metrics bullet (currently lines 212–213, "Out of scope" section) to a new "Implementation Status" appendix at the end of the ADR. The new appendix should read: "Operational metrics (outbox lag, drain-pass duration, wake-coalesce, drain startup, broadcast watermark, min_pending_seq): shipped in `docs/design-plans/<DATE>-phase-5-operational-metrics.md`. See `docs/architecture/backend-server.md` § Operational metrics for the inventory. The original ADR text said 'replay duration' — the implemented metric is `atc_pg_drain_startup_seconds` and measures startup-init latency, not replay backlog (Phase 3c restart-recovery contract precludes a replay backlog)."
  - The remaining "Out of scope" bullets (raw webhook persistence, leader election, Helm gating revisits) stay in place — those are still out of scope for this metrics chunk.
- `docs/architecture/state-externalization-research/rollout-and-implementation.md`:
  - In the Phase 5 section (currently lines 228–238), append a "Status" line for the metrics bullet: "Metrics: Done (date). See `docs/design-plans/<DATE>-phase-5-operational-metrics.md`."
  - The other three Phase 5 bullets stay open with their existing wording; add a parenthetical "(separate plan)" to each.
- `backend/crates/atc-server/CLAUDE.md`:
  - Extend the metric inventory section to list the six new metrics with one-line descriptions. Cross-link to `docs/architecture/backend-server.md` § Operational metrics rather than duplicating the per-metric details (per the non-duplication rule).
  - Stamp `Last verified:` with the implementation landing date.
- `CLAUDE.md` (root):
  - Update the Status paragraph: append "Phase 5 metrics chunk done — six operational metrics (outbox lag, drain-pass duration, wake-coalesce, drain startup, watermark, min_pending_seq) shipped with per-metric documentation and Grafana template dashboard."
  - Stamp `Last verified:` with the implementation landing date.

**ADR-driven stale-content sweep** (per `docs/implementation-guidance.md` Rule 6 — Phase 5 implements ADR 0002 deferral). Search the repo for stale "operational metrics: deferred to Phase 5" language and update each site:
- `docs/architecture/state-externalization-research/README.md` — search for "Phase 5" and reflect the new status.
- Any Phase 2d / 3c plans that reference "metrics will come in Phase 5" — annotate with a forward link to this plan.

### Step 6 — CONTRIBUTING.md "Metrics" section

Append a "Metrics" section to `CONTRIBUTING.md` (likely in the Documentation Conventions area). The section covers two things: the naming convention, and the cross-link to the canonical authoring contract. Content (literal, not pattern):

> ### Metrics
>
> ATC exposes Prometheus metrics at `/metrics`. Two rules apply when adding or modifying metrics:
>
> **Naming convention:**
> - `atc_` project prefix on every metric
> - `pg_` subsystem prefix for Postgres-path metrics; reserve future subsystem prefixes (`http_`, `ws_`, etc.) for analogous separation
> - `_total` suffix for monotonic counters
> - `_seconds` suffix for time-valued metrics regardless of metric type (counter, gauge, histogram). Prometheus best practice — `process_start_time_seconds` is a gauge; `axum_http_requests_duration_seconds` is a histogram
> - `_bytes` suffix for memory or byte-valued metrics
> - Gauges that aren't time/byte-valued carry no unit suffix; the description names the unit
> - No replica or pod label is baked into the metric — replica identity is added by the monitoring stack at scrape time as standard target labels (e.g., `pod`, `instance`)
>
> **Authoring contract:** every metric ships with the seven-element interpretation-surface block (name, type, labels with source, semantics, per-replica scope, aggregation guidance, example PromQL) in `docs/architecture/backend-server.md` § "Metric authoring contract". The contract is canonically defined there — this section codifies the rule that contributors who add metrics MUST extend that section before merge.

This step is small enough to land inline; no sub-agent dispatch needed.

### Step 7 — Verification

Use specific recipes (verified against current `justfile`):

- `just lint` — clippy clean (`cargo clippy --workspace --all-targets`).
- `just test` — full Rust + frontend tests pass (`cargo test --workspace` + `pnpm exec vitest run`); the six new tests are GREEN.
- `just helm-lint` — chart still lints clean (no chart change, but defensive).
- `just helm-unittest` — all existing helm-unittest cases still pass.
- `jq -e . deploy/grafana/atc-postgres-overview.json > /dev/null` — Grafana JSON parses.
- Manual: `just dev` in PG mode (against a local Postgres); `curl localhost:9090/metrics | grep '^atc_pg_'` shows all 15 `atc_pg_*` metrics (9 existing counters + 6 new). The `/metrics` endpoint binds on `ATC_METRICS_ADDR` (default `0.0.0.0:9090`), separate from the API port.
- Manual: load Grafana JSON, confirm all panels render. Capture screenshots for the PR test plan.
- Per memory `feedback_dont_skip_runtime_verification.md`: if the manual verification fails for environmental reasons, investigate and find a workaround; do not silently skip.

### Step 8 — PR and ADR-ref close

- Open PR with title `feat(server): add operational metrics for the postgres drain path`.
- PR body = squash commit body (per memory `feedback_pr_body_convention.md`): describe the six metrics, the doc + dashboard deliverables, and the ADR 0002 status sweep.
- Test plan as the FIRST PR comment (per memory `feedback_test_plans.md`): include `cargo test` output for the six new tests, the `/metrics` scrape excerpt, and the Grafana dashboard screenshots.
- No issue to close (Phase 5 is internal phasing, not an issue-tracked deliverable in this scope).

## Acceptance Criteria

| ID | Type | Criterion |
|----|------|-----------|
| **AC1** | Success | After a single webhook POST and the resulting drain pass, a `render_metrics()` capture shows `atc_pg_outbox_lag_seconds_count` increment by 1 and `_sum` increment in the range `[0.0, 5.0]` seconds (allowing CI machine slop while a 2-second sleep was in the test scenario). Backend test `metrics_outbox_lag_test.rs` asserts the count delta and the sum-delta range. |
| **AC2** | Failure | A separate test in the same file pre-seeds an outbox row via direct SQL with `inserted_at = NOW() + INTERVAL '10 minutes'`, drives the drain pass to broadcast it, and captures `render_metrics()`. Assert `_count` increments by 1 — the metric recorded an observation despite the unusual input, confirming no panic and no input-side clamping. The `_sum` behavior under negative observations depends on `metrics-exporter-prometheus`'s handling (which may accumulate negatives or filter them); the test does NOT assert on `_sum` for the negative case to keep the assertion robust to exporter behavior. The behavioral contract this test enforces: the lag computation reaches the histogram's `record()` path and the histogram remains usable after a sentinel-class observation. |
| **AC3** | Success | A `/metrics` scrape after one drain pass shows `atc_pg_drain_pass_duration_seconds_count` increment by 1 and `_sum` increment by a value in `[0.0001, 1.0]` seconds. Backend test `metrics_drain_pass_duration_test.rs` asserts this. |
| **AC3b** | Failure | A `TestApp` that runs idle (no webhooks for the duration of the test, only heartbeat ticks fire) shows `atc_pg_drain_pass_duration_seconds_count` delta of 0 (heartbeat-only wakes do NOT execute a drain pass per `listener.rs:168-176`). The metric is bound to NOTIFY-driven passes only. |
| **AC4** | Success | A controlled scenario where 5 webhooks are POSTed within a slow-drain window (`drain_delay=200ms` fixture from Phase 2d) results in `atc_pg_wake_coalesced_total` increment of at most 5 (no over-counting). The lower bound is intentionally 0 — without deterministic synchronization between listener task and drain task scheduling, the test cannot guarantee any specific NOTIFY observed `drain_in_flight=true`; the AC enforces "the counter exists, increments correctly, never over-counts" and pairs with AC5 to detect a stuck-true bug. |
| **AC5** | Failure | A scenario where webhooks are POSTed with full drain completion between each (no overlap with an in-flight pass) results in `atc_pg_wake_coalesced_total` delta of 0. Pairs with AC4: AC5 asserts the counter doesn't fire spuriously; AC4 asserts it doesn't over-count. Together they enforce the counter's correctness without a flaky lower-bound assertion. |
| **AC6** | Success | After constructing a fresh `TestApp` and waiting on the existing `drain_started: Arc<Notify>` first-pass signal, the delta of `atc_pg_drain_startup_seconds_count` across the test (post-test minus pre-test baseline) equals 1, and the `_sum` delta is `> 0`. Backend test `metrics_drain_startup_test.rs` asserts both deltas. |
| **AC6b** | Failure | After the first drain pass observation, subsequent drain passes (driven by additional webhooks in the same `TestApp`) do NOT add further `atc_pg_drain_startup_seconds_count` observations: the delta from "after first pass" to "after Nth pass" is 0. The metric fires once per process lifetime; this asserts the once-only contract. |
| **AC7** | Success | After 3 webhooks have been processed and drained, `atc_pg_broadcast_watermark` gauge value equals `MAX(seq)` from the outbox table (verified via parallel SQL query in the test). Backend test `metrics_broadcast_watermark_test.rs` asserts equality. |
| **AC7b** | Failure | Before any webhook is POSTed in a fresh `TestApp`, `atc_pg_broadcast_watermark` gauge value equals 0 (the seed value from `main.rs:151`'s `Arc::new(AtomicI64::new(0))`, mirrored by the gauge after the watermark seed at `main.rs:205`). The gauge is initialized at startup, not at first-broadcast. |
| **AC8** | Success | After aborting the drain task (so no swap-to-NaN ever happens) and emitting `pg_notify('atc_outbox', '<seq>')`, the listener's `fetch_min` mirrors a finite seq into the gauge. A `render_metrics()` scrape shows `atc_pg_min_pending_seq` is a finite numeric value below the current `atc_pg_broadcast_watermark` gauge. The test does not race the drain — it removes the drain from the runtime entirely. |
| **AC8b** | Failure | In a steady-state scenario (no gap-healing, drain has fully caught up), `atc_pg_min_pending_seq` gauge is NaN (the sentinel) — not a numeric value below watermark. The test scrapes `/metrics` after the drain has completed N webhooks with no concurrent-tx interference and asserts the gauge is `NaN`. |
| **AC9** | Success | `docs/architecture/backend-server.md` contains an "Operational metrics" section with: (a) the per-replica scoping paragraph, (b) per-metric subsections for all six new metrics with the seven required elements (Name, Type, Labels, Measures, Per-replica, Aggregation, Example PromQL), (c) one row added to the startup-behavior table referencing `atc_pg_drain_startup_seconds` timing. |
| **AC9a** | Success | `docs/architecture/backend-server.md` § Metrics has a "Metric authoring contract" subsection at the top of the metrics section, codifying the seven-element interpretation-surface block as a forward-binding rule that applies to every metric added to the codebase (not just Phase 5's). The subsection cross-references the doc-staleness gate. |
| **AC10** | Success | `deploy/grafana/atc-postgres-overview.json` exists; `jq -e . deploy/grafana/atc-postgres-overview.json > /dev/null` returns 0; the JSON's panel array contains at least six panels covering all six new metrics (one canonical query per metric per D8). The dashboard has been manually loaded into a real Grafana instance against a PG-mode replica generating data, and screenshots are captured in the PR's first-comment test plan. |
| **AC11** | Success | ADR 0002 has an "Implementation Status" appendix referencing the Phase 5 metrics design plan. The "Out of scope" bullet on operational metrics has been removed from that section. The other "Out of scope" bullets (raw webhook persistence, leader election, etc.) remain. |
| **AC12** | Success | `state-externalization-research/rollout-and-implementation.md` Phase 5 section's metrics bullet is annotated with "Done (DATE)" and a cross-link to this plan. The other three Phase 5 bullets are annotated with "(separate plan)" markers. |
| **AC13** | Success | `backend/crates/atc-server/CLAUDE.md` metric inventory references the six new metrics with one-line descriptions and a cross-link to `docs/architecture/backend-server.md` § Operational metrics. `Last verified:` stamped with the landing date. |
| **AC14** | Success | Root `CLAUDE.md` Status paragraph reflects the Phase 5 metrics chunk done with the landing-date `Last verified:` stamp. |
| **AC15** | Success | `CONTRIBUTING.md` has a "Metrics" section covering both the naming convention (the seven-bullet block from Step 6 — `atc_` prefix, subsystem prefix, `_total` for counters, `_seconds` for time-valued, `_bytes` for memory-valued, no-suffix for other gauges, no per-replica label) AND a cross-link to `docs/architecture/backend-server.md` § "Metric authoring contract" as the canonical home of the interpretation-surface rule. |
| **AC16** | Success | `just lint` and `just test` pass with the new tests included. No existing tests regressed. |

## Documents to Update

| Doc | Update |
|-----|--------|
| `docs/architecture/backend-server.md` | Add "Metric authoring contract" subsection (durable convention); add "Operational metrics" subsection with six per-metric seven-element blocks; add per-replica scoping prose; add startup-behavior row for replay timing; stamp `Last verified:` |
| `docs/architecture-decisions/0002-state-externalization-postgres-outbox.md` | Move operational-metrics bullet from "Out of scope" → new "Implementation Status" appendix |
| `docs/architecture/state-externalization-research/rollout-and-implementation.md` | Annotate Phase 5 metrics bullet "Done"; mark remaining bullets "(separate plan)" |
| `backend/crates/atc-server/CLAUDE.md` | Add six new metrics to inventory with cross-link; stamp `Last verified:` |
| `CLAUDE.md` (root) | Status paragraph reflects metrics chunk done; stamp `Last verified:` |
| `CONTRIBUTING.md` | Add "Metrics" section covering naming convention AND cross-link to backend-server.md § "Metric authoring contract" |

**Stale-content sweep targets** (per Step 5's ADR-driven sweep): `docs/architecture/state-externalization-research/README.md` (any "metrics in Phase 5" language updates to reflect status); Phase 2d / 3c plans referencing future Phase 5 metrics (forward-link annotation). Run `git grep -in "metrics.*Phase 5\|Phase 5.*metrics" -- ':!docs/design-plans'` to find any sites missed (excluding `docs/design-plans/` so the committed copy of this plan does not self-match).

`scripts/doc-mapping.sh` — verify only; no changes expected (`backend/crates/atc-server/src/*` → `backend-server.md` mapping covers all backend changes; `deploy/grafana/` is a new directory that does not need a doc-staleness mapping).

`deploy/helm/atc/*` — verified no change required (D9). Helm chart is metric-agnostic.

`.github/workflows/*` — verified no change required (D9). CI matrix already runs `cargo test --workspace`.

## Implementation Guidance

`docs/implementation-guidance.md` governs all implementation work for this plan. Specific rules and project-memory feedback that bite for Phase 5:

- **Rule 1 — feature branch + PR conventions.** Branch off `main`; squash-merge; PR title is `feat(server): add operational metrics for the postgres drain path`; test plan goes in the FIRST PR comment, not the PR body.
- **Rule 2 — TDD discipline.** Step 1 lists the six failing integration tests that anchor the implementation in Step 2. Run `just test` after Step 1 (RED), again after Step 2 (GREEN). All six tests use the existing `render_metrics()` delta pattern; none are source-grep tests.
- **Rule 3 — `just setup` at session start.** Per memory `feedback_verify_lefthook_installed.md`. Lefthook hooks must be installed in the worktree or formatting issues only surface in CI.
- **Rule 4 — doc-mapping.** Verified: `backend/crates/atc-server/src/*` → `docs/architecture/backend-server.md` mapping already exists. No new mapping entries.
- **Rule 5 — GitHub Actions SHA-pinning.** No CI workflow changes in this plan; if implementation discovers a needed `uses:` change, pin to a full SHA.
- **Rule 6 — ADR annotation sweep.** Phase 5 implements the ADR 0002 deferral on operational metrics. Step 5's stale-content sweep IS this rule's application.
- **Rule 14 — use subagents.** Step 3 (per-metric doc) and Step 5 (ADR/CLAUDE.md updates) dispatch to `project-claude-librarian`. Step 1 (test writing) and Step 2 (implementation) stay in the main context due to cross-file invariants.

**Memory-anchored conventions** to honor:

- `feedback_pr_title_convention.md` — full deliverable title; no first-commit-only titles.
- `feedback_test_plans.md` — test plan as first PR comment.
- `feedback_pr_body_convention.md` — PR body is the squash commit body; written as "what will be implemented" at design time, updated to "what was implemented" at finalize.
- `feedback_phases_not_in_user_facing_strings.md` — metric names are user-visible (operators see them on dashboards); none of the six metric names contain "phase 5" or "ADR 0002" — verified. The architecture doc's "Operational metrics" table heading does NOT use phase markers (forward-facing, dev-facing-but-read-by-operators surface). Phase markers stay only in this plan, the ADR appendix, and CLAUDE.md / AGENTS.md (dev-facing).
- `feedback_no_source_grep_tests.md` — every AC test scrapes `/metrics` and asserts observed values; none grep source files.
- `feedback_dont_assume_dep_minimalism.md` — no dependency framing as "minimal" or "fewer crates"; the existing `metrics` + `axum_prometheus` stack is locked.
- `feedback_dont_skip_runtime_verification.md` — manual `/metrics` scrape and Grafana dashboard load are mandatory verifications; do not silently skip on environmental noise.
- `feedback_verify_just_recipes_before_citing.md` — `just test`, `just lint`, `just helm-lint`, `just helm-unittest` verified against current justfile; cite no others without re-verifying.
- `feedback_verify_lefthook_installed.md` — `just setup` at session start.
- `feedback_plans_in_repo_no_review_artifacts.md` — design plans committed to `docs/design-plans/` ship as final documents.
- `feedback_fix_class_not_instance.md` — if implementation finds a structural bug pattern (e.g., "this metric would also benefit from buckets"), apply the fix to every analogous site, not just the one a reviewer flags.
- `feedback_verify_invariant_layer.md` — when the implementation cites "X is safe because Y has property Z," verify Y's layer actually has that property in the runtime path being traversed.

## Out of Scope (deferred)

- **Outbox retention / eviction strategy** — separate Phase 5 sub-plan; needs a design call before code (retention duration, deletion strategy, foreign-key implications). Per ADR 0003 Decision 4.
- **In-memory mode removal decision** — separate Phase 5 sub-plan; ADR-shape question (is `pg_pool: None` mode removed entirely or kept as a documented dev-only path?). Per ADR 0003 "Out of scope".
- **Persisting raw GitHub webhook JSON for audit** — separate Phase 5 sub-plan; per ADR 0002 "Out of scope" (still deferred after this plan moves the operational-metrics bullet out).
- **HPA / PDB / anti-affinity chart defaults** — issues #8 / #9 / #10; chart-track plan separate from metrics work. Unblocked by Phase 4.
- **kind-in-CI for chart smoke testing** — issue #12.
- **NetworkPolicy template** — issue #11.
- **Server-side leader election** — explicitly rejected as the multi-replica enabling mechanism per ADR 0002 Decision 5; not revisited.
- **Custom histogram buckets for `atc_pg_drain_pass_duration_seconds` and `atc_pg_outbox_lag_seconds`** — defaults `[0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10]` are sufficient for typical 1–500ms ranges. Only `atc_pg_drain_startup_seconds` gets custom buckets (the cold-pool tail is the relevant signal). Custom buckets for the other two land only if observed data shows the defaults are inadequate.
- **Prometheus native histograms migration** — `metrics-exporter-prometheus` 0.18.1 supports native histograms via `set_native_histogram_for_metric`, but emission requires switching `/metrics` exposition from text format to protobuf (or content-negotiated). Tracked as part of issue #59 (OTel instrumentation), which pivots metrics emission to OTel SDK + OTLP — exponential histograms travel naturally through OTLP collector to Prometheus remote-write as native histograms, removing the need for an in-process protobuf exposition refactor. The bucket-tuning concern in this plan becomes obsolete on that migration.
- **Templating the Grafana dashboard as a Helm ConfigMap** — Phase 5 ships the JSON as a standalone artifact. ConfigMap-bundling is a chart-track enhancement that can land in a follow-up if operators want it.
- **Per-replica labeling baked into metric names** — explicit reject in D1; replica identity is added by Prometheus's standard `pod`/`instance` labels at scrape time.
- **Metric for per-row drain throughput (rows/sec) as a derived metric** — derivable from `rate(atc_pg_drain_rows_total[5m])` in PromQL. No new metric needed.
- **Backfilling seven-element interpretation-surface blocks for the existing nine `atc_pg_*` counters** (write failures, drift, NOTIFY emit/recv, listener errors, drain passes/rows, duplicate skipped, unknown kind). The metric authoring contract is forward-binding from this plan's merge date; backfilling existing metrics is a doc-debt item for a follow-up doc-only PR. The existing LISTEN/NOTIFY metrics table at `backend-server.md:251–263` continues to serve as the inventory until then.
- **Pool/network failure injection for backend tests** — issue #56.
- **CI runner-disk optimization** — issue #55.

## Glossary

- **Drain pass.** One iteration of the drain task's loop: a single `notify.notified()` wake, followed by paginated SELECT batches over the outbox until exhausted, ending with a `broadcast_watermark.store()`.
- **Coalesced wake-up.** A NOTIFY arrival that the listener observes while `drain_in_flight=true`. Tokio's `Notify` permit semantics collapse multiple concurrent notifies into one extra pass; this metric counts the *arrival rate*, not the *extra-pass rate*.
- **Drain startup duration.** Wall time from `COALESCE(MAX(seq),0)` watermark init through first drain pass exit. Exactly one observation per process lifetime. Per Phase 3c restart-recovery contract there is no historical replay — the metric measures startup readiness latency, not catch-up backlog.
- **Outbox lag (event age at broadcast).** Histogram observation `now() - inserted_at` recorded for every broadcast outbox row. The metric is more accurately "event age at broadcast" than "drain lag": `inserted_at DEFAULT now()` evaluates to `transaction_timestamp()` (transaction start), so the metric includes writer-side transaction latency in addition to drain queueing. Under lock contention or long writer transactions the metric can materially overstate replica drain lag. Operators reading the p99 / p95 of this histogram should interpret it as "how stale is a typical row at broadcast time" rather than "how far behind is my drain task." The histogram type was chosen over a gauge so per-event observations contribute to the distribution rather than racing into a scrape-window snapshot.
- **Per-replica scope.** A metric whose semantic value is a property of one process (e.g., `broadcast_watermark` is per-replica because each replica has its own `Arc<AtomicI64>`). All Phase 5 metrics are per-replica; replica identity is added by Prometheus at scrape time.
- **`min_pending_seq`.** Per-replica `Arc<AtomicI64>` (init `i64::MAX`) updated by the listener on each NOTIFY via `fetch_min(seq)`. Drain task atomically swaps to `i64::MAX` at pass start and uses `min(watermark, swapped - 1)` as the actual query lower bound. Diverges below `broadcast_watermark` only during gap-healing rescans.
- **Scrape-injected label.** A Prometheus label added by the scraper (ServiceMonitor + Prometheus relabeling rules) at scrape time, not emitted by the application. `pod` and `instance` are scrape-injected.
