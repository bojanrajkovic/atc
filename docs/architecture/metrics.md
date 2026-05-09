# Metrics — `/metrics` endpoint surface

Last verified: 2026-05-08 (#61 sweep: confirmed zero phase nomenclature in this doc and re-verified per-metric prose against `metrics.rs`, `listener.rs`, and `persist.rs` after their description-string and comment cleanup.)

## Purpose

The `/metrics` endpoint exposes Prometheus text-format scrape data on a separate TCP listener (default `0.0.0.0:9090`, overridden via `ATC_METRICS_ADDR`). Serving metrics on a dedicated port keeps the metrics surface out of the application ingress and lets Kubernetes `NetworkPolicy` rules grant scrape access to Prometheus without exposing the full API.

This document is the canonical home for every metric ATC emits. New metrics must land here with the seven-element block defined under [Metric authoring contract](#metric-authoring-contract); cross-references from other docs (architecture writeups, deployment runbooks, Grafana panel descriptions, alert rules) should link to this file rather than duplicate the per-metric prose.

## Metric authoring contract

Every metric exposed at `/metrics` MUST ship with documentation in this section covering its interpretation surface — the contextual information an operator needs to read alerts, build dashboards, and decide which aggregator to use. Specifically, every metric documents:

1. **Name** — exact metric family name as scraped.
2. **Type** — counter / gauge / histogram.
3. **Labels** — every label name AND its source. Distinguish *emitted* labels (added by the application) from *scrape-injected* labels (e.g., `pod`, `instance`, added by the ServiceMonitor at scrape time).
4. **Measures** — one sentence stating what the metric value means in operational terms (not implementation terms).
5. **Per-replica vs cluster scope** — is the value a property of one replica's process state, or a cluster-wide invariant? This determines whether dashboards aggregate `by (pod)` or `without (pod)`.
6. **Aggregation guidance** — recommended cross-replica aggregator (`avg`/`max`/`sum`/`p99`) with one-sentence rationale.
7. **Example PromQL** — one canonical query that operators can copy-paste into Grafana to see meaningful data.

This contract applies to every metric added to the codebase, not just Postgres-path metrics. Plans that add metrics MUST extend the [Operational metrics](#operational-metrics) section with the new metric's seven-element block before merge. The doc-staleness gate (`scripts/check-docs-lefthook.sh`) enforces that backend metric changes must update `metrics.md`; this contract narrows the requirement from "update the doc" to "update the doc with the seven-element block."

## axum-prometheus placement

`PrometheusMetricLayer` wraps the main API router (not the metrics router). Every request to `http_addr` is counted in `axum_http_requests_total` and timed in `axum_http_requests_duration_seconds`. The metrics router itself is never wrapped — scrape requests do not appear in request metrics.

`metrics::build()` installs the global `metrics` recorder explicitly via `PrometheusBuilder::install_recorder()` and spawns the 5-second `run_upkeep()` loop manually (axum-prometheus's `pair()` would do this internally, but the explicit install lets us register custom histogram buckets first). `PrometheusMetricLayer::new()` does not install a recorder; it records to the global one we installed. The build path registers two bucket overrides:

- `Matcher::Full("atc_pg_drain_startup_seconds")` — custom buckets `[0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]` covering typical 50ms–10s startup latency.
- `Matcher::Suffix("_seconds")` — `axum_prometheus::utils::SECONDS_DURATION_BUCKETS`, the standard `[0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10]` distribution. Without this fallback, `metrics-exporter-prometheus` 0.18 emits unmatched histograms as Summary (no `_bucket` lines) and the `axum_http_requests_duration_seconds_bucket`, `atc_pg_outbox_lag_seconds_bucket`, and `atc_pg_drain_pass_duration_seconds_bucket` series would not appear.

## atc_build_info labels

`register_build_info()` (called once at startup) sets a gauge always equal to `1.0` with these labels:

| Label | Source | Example |
|---|---|---|
| `version` | `CARGO_PKG_VERSION` | `0.2.0` |
| `git_sha` | `VERGEN_GIT_SHA` (via `build.rs`) | `a1b2c3d...` |
| `rustc_version` | `VERGEN_RUSTC_SEMVER` (via `build.rs`) | `1.94.0` |
| `build_timestamp` | `VERGEN_BUILD_TIMESTAMP` (via `build.rs`) | `2026-04-08T...` |
| `target_triple` | `VERGEN_CARGO_TARGET_TRIPLE` (via `build.rs`) | `x86_64-unknown-linux-gnu` |

`build.rs` uses the `vergen-gix` crate (pure-Rust gix backend; no libgit2 dependency) and emits all five vars as `cargo:rustc-env=` instructions.

## Process collector

`spawn_process_collector()` starts a detached tokio task that calls `metrics_process::Collector::default().collect()` every 10 seconds. It uses the same global recorder installed by axum-prometheus. Emitted families include `process_cpu_seconds_total`, `process_resident_memory_bytes`, `process_virtual_memory_bytes`, `process_open_fds`, `process_max_fds`, `process_start_time_seconds`, and `process_threads`.

## Operational metrics

All `atc_pg_*` metrics are emitted unlabeled per-process. Replica identity is added by the monitoring stack at scrape time as standard target labels (`pod`, `instance`) — the exact attachment mechanism depends on the deployment (Prometheus Operator ServiceMonitor, plain Prometheus with `kubernetes_sd_configs`, VictoriaMetrics, etc.); the metrics themselves are agnostic. Cross-replica aggregation in alerts and dashboards uses `avg by (pod)`, `max by (pod)`, etc.

The blocks below are listed in roughly the order an event traverses the pipeline: webhook write → outbox row → NOTIFY emission → listener receipt → drain pass → broadcast → snapshot cursor.

### `atc_pg_write_failures_total`

- **Name:** `atc_pg_write_failures_total`
- **Type:** counter
- **Labels:** emitted `kind` ∈ `{parity, transient}`; scrape-injected `pod`, `instance`. `kind="parity"` fires when the PG UPSERT matches 0 rows (the WHERE predicate rejected the transition under PG's view of state); `kind="transient"` fires on sqlx errors at `pool.begin()`, mid-transaction, or `tx.commit()`.
- **Measures:** Webhook writes that failed inside `PgStore::apply_*_event`. Parity rejections return a 200 `{"status":"rejected"}` to GitHub and are NOT retried. Transient failures return 503 and ARE retried by GitHub's webhook delivery. Sustained nonzero rates of either kind indicate a real problem: parity means state-machine drift between PG and the in-memory model (page-worthy); transient means the database path is unhealthy.
- **Per-replica vs cluster:** Per-replica — only the writer replica increments. In multi-replica deployments any single replica can be the writer for a given webhook (GitHub picks one ingress).
- **Aggregation:** `sum by (kind)` cluster-wide for severity routing (parity → page; transient → alert on sustained rate). `max by (pod)` to localize a misbehaving replica.
- **Example PromQL:** `sum by (kind) (rate(atc_pg_write_failures_total[5m]))`

### `atc_pg_in_memory_drift_total`

- **Name:** `atc_pg_in_memory_drift_total`
- **Type:** counter
- **Labels:** none emitted; `pod`, `instance` (scrape-injected).
- **Measures:** Events where the PG transaction committed successfully but the in-memory `RunStateMachine` apply on the same replica subsequently diverged. The committed PG row is durable and recoverable from the outbox, so a single increment is not data loss — but a sustained rate signals a code defect in the in-memory state machine and warrants a page.
- **Per-replica vs cluster:** Per-replica observation; cluster-relevant signal because any replica's drift indicates a logic bug independent of which one observed it.
- **Aggregation:** `sum without (pod, instance)` for cluster-wide drift rate; alert on any nonzero sustained rate.
- **Example PromQL:** `sum(rate(atc_pg_in_memory_drift_total[5m]))`

### `atc_pg_notify_emitted_total`

- **Name:** `atc_pg_notify_emitted_total`
- **Type:** counter
- **Labels:** emitted `kind` ∈ `{run, job}` matching the event discriminator; scrape-injected `pod`, `instance`. Incremented by `PgStore::apply_*_event` after `tx.commit()` succeeds (the in-transaction `pg_notify` call is queued by PG and delivered on commit; aborted transactions silently drop it, so this counter only increments when the NOTIFY actually went out).
- **Measures:** Successfully committed write transactions broadcast to `LISTEN atc_outbox`. This is the writer-side "what was published" signal; the listener-side counterpart is `atc_pg_notify_received_total`.
- **Per-replica vs cluster:** Per-replica (only the writer replica increments for a given seq). Cluster-wide ingestion volume is the useful aggregation; per-replica view is rarely meaningful.
- **Aggregation:** `sum by (kind) (rate(...))` for cluster ingestion rate split by event kind. Use `sum without (pod, instance)` if you do not care about kind.
- **Example PromQL:** `sum by (kind) (rate(atc_pg_notify_emitted_total[5m]))`

### `atc_pg_notify_received_total`

- **Name:** `atc_pg_notify_received_total`
- **Type:** counter
- **Labels:** none emitted; `pod`, `instance` (scrape-injected).
- **Measures:** NOTIFY payloads received by this replica's listener task on the `atc_outbox` channel. Every replica's listener receives every NOTIFY (PG fans out to all sessions holding `LISTEN atc_outbox`), so the per-replica rate should track parity across replicas. A replica whose rate falls behind the others has a stuck or stalled listener.
- **Per-replica vs cluster:** Per-replica.
- **Aggregation:** `avg by (pod) (rate(...))` to verify parity across replicas; `min by (pod) (rate(...))` to flag a replica whose listener is stuck. Sqlx hides successful reconnects, so a counter that briefly plateaus and then catches up is a normal reconnect; a counter that stops without resuming is a stuck listener.
- **Example PromQL:** `rate(atc_pg_notify_received_total[5m])`

### `atc_pg_listener_recv_errors_total`

- **Name:** `atc_pg_listener_recv_errors_total`
- **Type:** counter
- **Labels:** none emitted; `pod`, `instance` (scrape-injected).
- **Measures:** Receive errors surfaced by the listener task (e.g., connection drops that sqlx could not silently reconnect through). Sqlx attempts to reconnect transparently on most listener errors; this counter only fires when the error escapes that retry loop. A nonzero rate over more than a single scrape window means the listener is repeatedly failing to recover.
- **Per-replica vs cluster:** Per-replica.
- **Aggregation:** `max by (pod) (rate(...))` — a single misbehaving replica is the actionable signal; sustained nonzero rate on any pod warrants investigation (likely DSN / session-mode misconfiguration; see `backend-server.md` § "DSN session-mode contract").
- **Example PromQL:** `rate(atc_pg_listener_recv_errors_total[5m])`

### `atc_pg_drain_passes_total`

- **Name:** `atc_pg_drain_passes_total`
- **Type:** counter
- **Labels:** none emitted; `pod`, `instance` (scrape-injected). Heartbeat-only wakes (the 5-second readiness tick that fires `last_drain_pass_at` updates without doing any draining) do NOT increment — only NOTIFY-driven passes count.
- **Measures:** NOTIFY-driven drain passes completed by this replica. A flat-zero rate during a period of nonzero `atc_pg_notify_received_total` indicates the drain task is wedged.
- **Per-replica vs cluster:** Per-replica.
- **Aggregation:** `rate(... [5m]) by (pod)` — verify that drain passes are running on every replica that is receiving NOTIFYs. Pair with `atc_pg_notify_received_total` for a "wake → drain" sanity check.
- **Example PromQL:** `rate(atc_pg_drain_passes_total[5m])`

### `atc_pg_drain_rows_total`

- **Name:** `atc_pg_drain_rows_total`
- **Type:** counter
- **Labels:** none emitted; `pod`, `instance` (scrape-injected).
- **Measures:** Outbox rows fetched and processed by the drain task across all paginated batches. Useful as a writer-vs-drain throughput sanity check: cluster-wide `rate(atc_pg_drain_rows_total)` summed across replicas should approximately equal `rate(atc_pg_notify_emitted_total)` × replica count over the same window (each replica's drain reads every committed row).
- **Per-replica vs cluster:** Per-replica.
- **Aggregation:** `sum by (pod)` per-replica; `sum without (pod, instance)` for cluster total.
- **Example PromQL:** `rate(atc_pg_drain_rows_total[5m])`

### `atc_pg_drain_duplicate_skipped_total`

- **Name:** `atc_pg_drain_duplicate_skipped_total`
- **Type:** counter
- **Labels:** none emitted; `pod`, `instance` (scrape-injected).
- **Measures:** Outbox rows fetched during a drain pass but suppressed by the ring-buffer dedup because they had already been broadcast in a previous pass. Nonzero rate is the gap-healing rescan signal: the drain re-fetched a range of seqs because a NOTIFY arrived for a seq below the local watermark, and dedup correctly suppressed re-broadcast. Brief nonzero values during reorder windows are normal; a sustained high rate means the drain is repeatedly rescanning the same range and indicates either backstop math drift or an upstream NOTIFY storm.
- **Per-replica vs cluster:** Per-replica.
- **Aggregation:** `max by (pod) (rate(...))` — sustained nonzero rate on any single replica is the actionable signal.
- **Example PromQL:** `rate(atc_pg_drain_duplicate_skipped_total[5m])`

### `atc_pg_drain_unknown_kind_total`

- **Name:** `atc_pg_drain_unknown_kind_total`
- **Type:** counter
- **Labels:** none emitted; `pod`, `instance` (scrape-injected).
- **Measures:** Outbox rows whose `kind` discriminator was neither `run` nor `job`. The set of legal kinds is fixed by a CHECK constraint on the outbox table, so this counter should be flat zero in any healthy deployment. A nonzero value is either a deploy-skew signal (an older replica writing a kind a newer replica does not understand, or vice versa) or a schema invariant violation; alert on first observation.
- **Per-replica vs cluster:** Per-replica observation; cluster-relevant signal.
- **Aggregation:** `sum without (pod, instance) (increase(...))` over a multi-hour window for the alert rule.
- **Example PromQL:** `increase(atc_pg_drain_unknown_kind_total[1h])`

### `atc_pg_outbox_lag_seconds`

- **Name:** `atc_pg_outbox_lag_seconds`
- **Type:** histogram
- **Labels:** none emitted; `pod`, `instance` added by the scraper (scrape-injected)
- **Measures:** Event age at broadcast — `Utc::now() - row.inserted_at` recorded once per broadcast row. The metric is more accurately "event age at broadcast" than "drain lag": `inserted_at DEFAULT now()` evaluates `transaction_timestamp()` (transaction start, not commit), so the metric includes writer-side transaction latency in addition to drain queueing. Operators reading p99/p95 should interpret it as "how stale is a typical row at broadcast time," not "how far behind is my drain task."
- **Per-replica vs cluster:** Per-replica — each replica's drain task records its own observations from its own broadcasts.
- **Aggregation:** `histogram_quantile(0.99, sum(rate(...)) by (le, pod))` then `max by (pod)` for alerting — the slowest replica is the operationally relevant signal because all replicas serve traffic.
- **Example PromQL:** `histogram_quantile(0.99, sum(rate(atc_pg_outbox_lag_seconds_bucket[5m])) by (le, pod))`

### `atc_pg_drain_pass_duration_seconds`

- **Name:** `atc_pg_drain_pass_duration_seconds`
- **Type:** histogram
- **Labels:** none emitted; `pod`, `instance` (scrape-injected)
- **Measures:** Wall time from drain-pass start to drain-pass exit, including all paginated batches in the pass. NOT recorded for heartbeat-only wakes.
- **Per-replica vs cluster:** Per-replica — drain runs independently on each replica.
- **Aggregation:** `histogram_quantile(0.99, ...)` `by (pod)` for per-replica latency; `avg by (pod)` for trend tracking.
- **Example PromQL:** `histogram_quantile(0.99, sum(rate(atc_pg_drain_pass_duration_seconds_bucket[5m])) by (le, pod))`

### `atc_pg_wake_coalesced_total`

- **Name:** `atc_pg_wake_coalesced_total`
- **Type:** counter
- **Labels:** none emitted; `pod`, `instance` (scrape-injected)
- **Measures:** NOTIFY arrivals observed by the listener while a drain pass was in flight (`drain_in_flight=true`). Counts arrival rate, NOT extra-pass rate (Tokio's `Notify` permit collapses N permits into 1 — the metric is about NOTIFY arrival vs drain-pass scheduling, which is what operators want).
- **Per-replica vs cluster:** Per-replica.
- **Aggregation:** `rate(... [5m]) by (pod)` then `max by (pod)` — sustained high values on any replica indicate a NOTIFY storm or slow drain.
- **Example PromQL:** `rate(atc_pg_wake_coalesced_total[5m])`

### `atc_pg_drain_startup_seconds`

- **Name:** `atc_pg_drain_startup_seconds`
- **Type:** histogram (custom buckets `[0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]`)
- **Labels:** none emitted; `pod`, `instance` (scrape-injected)
- **Measures:** Startup readiness latency — wall time from `COALESCE(MAX(seq),0)` watermark init through first drain pass exit. One observation per process lifetime. Per the restart-recovery contract there is no historical replay; this measures startup readiness, NOT catch-up backlog.
- **Per-replica vs cluster:** Per-replica.
- **Aggregation:** `max by (pod)` over a window covering recent deploys (1h) — the slowest replica's startup is the operational signal.
- **Example PromQL:** `histogram_quantile(0.99, sum(rate(atc_pg_drain_startup_seconds_bucket[1h])) by (le, pod))`

### `atc_pg_broadcast_watermark`

- **Name:** `atc_pg_broadcast_watermark`
- **Type:** gauge
- **Labels:** none emitted; `pod`, `instance` (scrape-injected)
- **Measures:** Highest outbox seq broadcast by this replica's drain task — the commit-order cursor read by `state_handler` as `lastSeq` in PG mode. Mirrors the per-replica `Arc<AtomicI64>` after each successful drain pass; seeded at startup from `COALESCE(MAX(seq),0)`.
- **Per-replica vs cluster:** Per-replica — each replica advances its watermark independently.
- **Aggregation:** Display per-pod (`atc_pg_broadcast_watermark`); for a single cluster-wide "laggiest replica" series, use `min(atc_pg_broadcast_watermark)` (or equivalently `min without (pod, instance)`). Note: `min by (pod) (atc_pg_broadcast_watermark)` would just preserve one series per pod — same as the per-pod display.
- **Example PromQL:** `atc_pg_broadcast_watermark`

### `atc_pg_min_pending_seq`

- **Name:** `atc_pg_min_pending_seq`
- **Type:** gauge
- **Labels:** none emitted; `pod`, `instance` (scrape-injected)
- **Measures:** Lowest pending NOTIFY seq below the watermark (the gap-healing pressure signal). Mirrors the per-replica `min_pending_seq: Arc<AtomicI64>` after each listener `fetch_min`; reset to `f64::NAN` (the sentinel state) when the drain swaps the atomic to `i64::MAX` after catching up. NaN is preferred over `i64::MAX as f64` (≈ 9.22e18) because the float64 representation would push the y-axis of dashboards displaying watermark and min_pending_seq together to ~9e18, hiding the actual divergence signal at the watermark level.
- **Per-replica vs cluster:** Per-replica.
- **Aggregation:** Display per-pod alongside `atc_pg_broadcast_watermark`. Filter NaN with `... unless on() (atc_pg_min_pending_seq != atc_pg_min_pending_seq)` if needed.
- **Example PromQL:** `atc_pg_min_pending_seq` (Grafana renders NaN as gaps)

## Listener always binds

The metrics listener binds unconditionally at startup regardless of the chart's `metrics.enabled` value. This is intentional: the chart flag controls whether Prometheus discovers the endpoint (via ServiceMonitor or pod annotations); the port is always open so that `kubectl port-forward` and ad-hoc `curl` work without chart-level changes.
