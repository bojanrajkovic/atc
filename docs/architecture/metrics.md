# Observability — metric and span surface

Last verified: 2026-05-14

> **Note on issue #16 (runner-pool capacities).** The wire field
> `StateSnapshot.runner_pool_capacities` carries operator-declared capacity
> from `AppState` onto the snapshot response. It is **not** a metric and
> introduces no new `atc_runner_pool_*` instrument — ADR 0004 keeps pool
> stats derivation on the frontend. See `docs/architecture-decisions/0004-frontend-derived-pool-stats.md` and the
> design plan at `docs/design-plans/2026-05-13-issue-16-runner-pool-capacity.md`.

## Purpose

ATC emits structured telemetry — metrics, spans, and JSON logs — through one OpenTelemetry pipeline. When `OTEL_EXPORTER_OTLP_ENDPOINT` is set, the SDK initializes a tracer provider and a meter provider that export over OTLP/HTTP (HTTP/protobuf) to an operator-run collector (Grafana Alloy, OpenTelemetry Collector, etc.). The collector decides the downstream destination — Tempo for traces, Mimir or Prometheus for metrics, Loki for logs — and re-exposes whichever scrape format the monitoring stack consumes. When the env var is unset, the SDK is never initialized: the OTel global meter provider stays at the SDK's no-op default, every instrument built against it is a no-op, `tracing` events flow only to the JSON / pretty stderr subscriber, and there is no provider, no exporter, and no background-task overhead. An invalid `OTEL_EXPORTER_OTLP_ENDPOINT` (typo, missing scheme, unparseable URL) is treated as unset — `init_otel` parses the value as a URI before installing the SDK so misconfiguration disables OTel with a clear stderr warning instead of silently routing telemetry to the OTel SDK's default `http://localhost:4318` fallback.

This document is the canonical home for ATC's metric and span contract. Every metric ATC emits is documented here with the seven-element interpretation block defined under [Metric and span authoring contract](#metric-and-span-authoring-contract). Every span boundary is enumerated under [Span inventory](#span-inventory). Cross-references from other docs (backend-server architecture, deployment runbooks, dashboard descriptions, alert rules) link here rather than duplicating per-metric or per-span prose.

## Metric and span authoring contract

Every metric ATC emits MUST ship with documentation in this section covering its interpretation surface — the contextual information an operator needs to read alerts, build dashboards, and decide which aggregator to use. Every span boundary ATC adds MUST be enumerated in [Span inventory](#span-inventory).

### Metric naming

- `atc_` project prefix on every application metric.
- `pg_` subsystem prefix for Postgres-path metrics; reserve future subsystem prefixes (`http_`, `ws_`, etc.) for analogous separation.
- `_total` suffix for monotonic counters.
- `_seconds` suffix for time-valued metrics regardless of metric type (counter, gauge, histogram). This follows Prometheus convention — `process_start_time_seconds` is a gauge, the HTTP request duration histogram is `_seconds`.
- `_bytes` suffix for memory or byte-valued metrics.
- Gauges that are not time- or byte-valued carry no unit suffix; the description names the unit.
- snake_case throughout. The OTLP→Prometheus path leaves names alone, so the production exposition matches the source.

### Metric attribute conventions

- Lowercase keys; no high-cardinality values; no PII.
- No replica or pod label is baked into any application metric — replica identity is added by the collector at ingest time as standard target labels (`pod`, `instance`).
- Use HTTP semantic conventions for HTTP-shaped metrics: `http.request.method`, `http.response.status_code`, `http.route`, `url.scheme`. The `axum-otel-metrics` middleware emits these on the request-duration histogram.

Every new metric MUST extend [Operational metrics](#operational-metrics) with the seven-element block before merge. The doc-staleness gate (`scripts/check-docs-lefthook.sh`) blocks the push if backend metric changes land without a matching update here.

The seven elements:

1. **Name** — exact metric name as exported.
2. **Type** — counter / gauge / histogram.
3. **Attributes** — every emitted attribute name AND its source. Distinguish *emitted* attributes (added by the application) from *injected* attributes (e.g., `pod`, `instance`, added by the collector at ingest).
4. **Measures** — one sentence stating what the metric value means in operational terms (not implementation terms).
5. **Per-replica vs cluster scope** — is the value a property of one replica's process state, or a cluster-wide invariant? This determines whether dashboards aggregate `by (pod)` or `without (pod)`.
6. **Aggregation guidance** — recommended cross-replica aggregator (`avg`/`max`/`sum`/`p99`) with one-sentence rationale.
7. **Example PromQL** — one canonical query an operator can paste into Grafana to see meaningful data. Queries assume the OTLP→Prometheus path (the collector translates OTel exponential histograms into Prometheus native histograms; see [Histogram aggregation](#histogram-aggregation) for the cross-format note on `*_bucket` series).

### Span naming

ATC spans use a dotted hierarchy that names the boundary, not the implementation:

- `state.snapshot` — root request span for `GET /v1/state` reads.
- `persist.read.snapshot` — `PgStore` / `InMemoryStore` read-path entry; child of `state.snapshot`.
- `webhook.handler` — root request span for `/v1/webhooks/github` POSTs.
- `webhook.verify`, `webhook.parse` — atc-github boundary spans nested under `webhook.handler`.
- `persist.apply.run_event`, `persist.apply.job_event` — `PgStore` / `InMemoryStore` write-path entries.
- `persist.notify.emit` — the in-transaction `pg_notify` after the outbox INSERT.
- `listener.task`, `listener.recv` — task-lifetime root and per-NOTIFY child for the PG listener.
- `drain.task`, `drain.pass`, `drain.broadcast` — task-lifetime root, per-pass child, per-row grandchild for the outbox drain.
- `eviction.sweep` — per-tick root span for the in-memory-mode TTL eviction sweep. Each `InMemoryStore::evict_expired` call is its own root (no task-lifetime parent) so every tick exports as one tidy trace.

Span names are stable identifiers — operators build dashboards and alerts that filter on them. Do not rename a span without coordinating with the dashboard owners; in particular, do not change `webhook.*`, `persist.*`, `listener.*`, or `drain.*` names without updating the doc here in lockstep.

### Span attribute conventions

- Lowercase, dotted keys (e.g., `webhook.delivery_id`, `pass.rows_fetched`). Use OpenTelemetry semantic conventions where they apply (`http.route`, `http.request.method`, `http.response.status_code`).
- Late-bound fields use `tracing::field::Empty` at span construction and `Span::current().record(...)` once the value is known. The `webhook.handler` span declares `webhook.delivery_id` and `webhook.event_type` as `Empty` and records them inside the handler body once the headers have been parsed.
- Never put webhook bodies, signatures, secrets, or full URLs (with secrets in query strings) on a span. The webhook attributes capture identifiers (`delivery_id`, `event_type`, `action`) and presence flags (`signature.present`), not payloads.

### Boundary discipline

New instrumentation goes at one of these boundaries:

- **API boundaries.** Every public HTTP route handler that performs work worth tracing (today: `webhook_handler`). The `axum-otel-metrics` middleware is the duration / status-code surface for *every* HTTP route automatically; per-route span instrumentation only needs to be added when the handler does enough work that the operator wants to see its internal structure.
- **Persist boundaries.** `PgStore::apply_*_event` and the in-transaction outbox / notify helpers under `persist/pg.rs`. Internal SQL helpers nested inside an `apply_*` span inherit context via the default `#[instrument]` skip rules.
- **Background-task boundaries.** Long-lived futures spawned with `tokio::spawn` need an explicit task-lifetime root span (`listener.task`, `drain.task`) constructed at spawn time and attached via `.instrument(span)` — see the [Tokio spawn gotcha](#tokio-spawn-gotcha) below.

Do NOT decorate every internal function with `#[tracing::instrument]`. Spans are an operator surface; not an internal call graph. If a function is not load-bearing for an operator reading a flame graph, leave it uninstrumented and let it inherit the surrounding span.

### Cached instrument convention

Every repeat-emit metric in `atc-server` MUST go through a cached OTel `Counter` / `Gauge` / `Histogram` instrument held on the `PgMetrics` struct in `backend/crates/atc-server/src/metrics.rs`. `PgStore::start` calls `PgMetrics::register()` once after the global meter provider is installed; the resulting `Arc<PgMetrics>` is cloned into the listener and drain task closures, and every emit on a hot path is a field access (`metrics.drain_rows.add(N, &[])`) instead of building an instrument inline at every call.

For attribute-bearing instruments (`atc_pg_write_failures_total{kind=…}`, `atc_pg_notify_emitted_total{kind=…}`), `PgMetrics` stores **one instrument per metric name** plus pre-built `[KeyValue; N]` attribute slices alongside it (e.g. `attrs_parity: [KeyValue; 1]`). Emit sites read `counter.add(1, &self.attrs_parity)` so neither the instrument lookup nor the `KeyValue` allocation happens on a webhook path. Dedicated helpers (`write_failure_parity()`, `notify_emitted_run()`, etc.) wrap each `(instrument, slice)` pair so call sites never duplicate the `&self.attrs_*` reference.

This is **defense-in-depth, not micro-perf**. The hash-contract correctness fix from PR #153 was rooted in `metrics-util` / `metrics-exporter-otel`; with both crates retired in favor of direct OTel meters that bug class is gone at the SDK seam. The cached-instrument pattern still earns its keep by keeping hot-path emits allocation-free and making the metric surface a single grep-able struct.

**TODO(otel-0.32):** once `tracing-opentelemetry` and `axum-otel-metrics` publish releases targeting `opentelemetry 0.32` (upstream PRs `tokio-rs/tracing-opentelemetry#258` and `ttys3/axum-otel-metrics#196`), bump the SDK pin, enable the `experimental_metrics_bound_instruments` feature, and replace each `(instrument, [KeyValue; N])` pair with a real `BoundCounter<u64>` / `BoundHistogram<f64>` obtained via `Counter::bind(&[…])`. Emit-site shape stays identical; the wrapper helpers (`write_failure_parity` etc.) hide the swap.

One inline exception remains: `register_build_info()` builds an `f64_gauge` and records `1.0` against it exactly once at startup with compile-time labels. The instrument is unique to that call site, so caching would be pure ceremony. Future contributors must not generalize this exception — it is a one-shot startup emit, not a "metadata-only" carve-out.

Mechanical check (run before merging changes that touch `atc-server/src/`):

```sh
rg -nU --multiline \
   'meter[^"]*"\)\s*\.(u64_counter|f64_(?:counter|gauge|histogram)|i64_(?:counter|gauge|histogram)|u64_(?:gauge|histogram))\([^)]+\)' \
   backend/crates/atc-server/src/ \
   | rg -v 'crates/atc-server/src/metrics\.rs'
```

The grep should return no matches: the only sites that build OTel instruments live inside `metrics.rs` (`register_build_info` plus `PgMetrics::register_with_meter`). A new hit anywhere else is a reintroduced inline emit and must be moved onto `PgMetrics` (or, if it represents a genuinely new metric, added to `PgMetrics::register_with_meter` and documented under [Operational metrics](#operational-metrics)).

`atc_pg_in_memory_drift_total` is registered in `PgMetrics::register_with_meter` but no field is cached: the metric is part of the documented surface but has no production emit site today. If a future writer adds an emit, the cached-instrument field MUST be added in the same change.

### W3C trace context propagation

The OTel SDK installs `TraceContextPropagator` globally in `init_otel`. The webhook handler extracts the incoming `traceparent` header before constructing the request span:

```rust
let parent_cx = opentelemetry::global::get_text_map_propagator(|prop| {
    prop.extract(&HeaderExtractor(&headers))
});
let span = info_span!("webhook.handler", /* ... */);
let _ = span.set_parent(parent_cx);
async move { /* handler body */ }.instrument(span).await
```

`set_parent` MUST be called between span construction and the first poll of the instrumented future — calling it from inside an `#[instrument]` body is wrong because the span has already been entered. If the header is absent or malformed, `parent_cx` is the empty context and the resulting `webhook.handler` span is a fresh root with a new trace ID. Outbound HTTP is not yet instrumented; if it is added later, the same propagator emits `traceparent` on outgoing requests.

### Tokio spawn gotcha

`tokio::spawn` does NOT propagate the calling task's parent span. A future spawned with `tokio::spawn(async { ... })` becomes a fresh root unless explicitly instrumented. For task-lifetime root spans (e.g., `listener.task`, `drain.task`), construct the span at spawn time and wrap the future via `.instrument(span)` from the `tracing::Instrument` trait:

```rust
let task_span = info_span!("drain.task");
tokio::spawn(
    async move { /* drain loop body */ }.instrument(task_span),
);
```

Per-iteration child spans (e.g., `drain.pass`, `drain.broadcast`) attach as descendants of the surrounding task span automatically because they are constructed inside the instrumented future. The same pattern wraps the listener task — `backend/crates/atc-server/src/listener.rs` is the reference implementation.

### Shutdown ordering

OTel SDK tear-down runs after every emitter has joined. The shutdown orchestration enumerates the ordering — see the comment block in `backend/crates/atc-server/src/shutdown.rs` (`run_shutdown_orchestration`, near the end). The principle is "no live emitter when shutdown fires." A new emitter category (a future scheduled job, a periodic task, a long-lived consumer) MUST be joined before the OTel shutdown step and named in that comment so the invariant continues to hold.

### Histogram aggregation

The meter provider registers an instrument view that maps every `Histogram` instrument to `Aggregation::Base2ExponentialHistogram { max_size, max_scale }`. The shared `exponential_histogram_view()` function in `backend/crates/atc-server/src/otel.rs` is the canonical hook for changing aggregation choice — both production `init_otel` and the test harness call it, so tests observe the same shape as production.

This requires the `spec_unstable_metrics_views` feature on `opentelemetry_sdk`. The feature is unstable per OTel's spec stability tracking — the API may shift on a future SDK release. The implementer who bumps `opentelemetry_sdk` owns reviewing the feature gate and, if it has been stabilized, removing the unstable flag.

The cross-format implication: when the OTLP collector translates an OTel exponential histogram into a Prometheus native histogram (Prometheus 2.40+), the resulting series does NOT emit `*_bucket` lines. Dashboards that previously queried `histogram_quantile(0.99, sum(rate(name_bucket[5m])) by (le, pod))` continue to work in Prometheus 2.40+ because `histogram_quantile` accepts native histograms directly. If the collector is configured to emit classic histograms instead (some operators run mixed stacks during migration), `*_bucket` series reappear and the same query continues to work.

## atc_build_info

`register_build_info()` (called once at startup) sets a gauge always equal to `1.0` with these labels (emitted as OTel attributes; rendered as Prometheus labels by the collector):

| Label | Source | Example |
|---|---|---|
| `version` | `CARGO_PKG_VERSION` | `0.2.0` |
| `git_describe` | `VERGEN_GIT_DESCRIBE` (via `build.rs`) | `v1.0.0` (exact tag for release-pipeline builds), `v1.0.0-3-gabc1234` (post-tag offset for local builds) |
| `git_sha` | `VERGEN_GIT_SHA` (via `build.rs`) | `a1b2c3d...` |
| `rustc_version` | `VERGEN_RUSTC_SEMVER` (via `build.rs`) | `1.94.0` |
| `build_timestamp` | `VERGEN_BUILD_TIMESTAMP` (via `build.rs`) | `2026-04-08T...` |
| `target_triple` | `VERGEN_CARGO_TARGET_TRIPLE` (via `build.rs`) | `x86_64-unknown-linux-gnu` |

`build.rs` uses the `vergen-gix` crate (pure-Rust gix backend; no libgit2 dependency) and emits all six vars as `cargo:rustc-env=` instructions. `release.yml`'s `actions/checkout` step uses `fetch-depth: 0` for the `build-binaries` job so vergen-gix's `git describe` walk has full ancestry (a shallow clone fetches the tag ref but not the history `git describe` traverses to find the nearest tag, and `VERGEN_GIT_DESCRIBE` falls back to an idempotent-output sentinel).

`main.rs` also emits an `atc-server starting` INFO log line at process startup carrying the same six fields. The log surfaces build metadata when the metrics endpoint isn't available — early startup crashes, OTel pipeline disabled, container logs as the only diagnostic surface.

## HTTP request duration

`axum-otel-metrics`'s `HttpMetricsLayer` wraps the API router in `routes::api_routes()`. Every request emits a duration histogram with HTTP semantic-convention attributes:

- `http.request.method` — request method (`POST`, `GET`, etc.).
- `http.response.status_code` — response status (`200`, `401`, `503`, etc.).
- `http.route` — matched Axum route pattern (`/v1/webhooks/github`, `/v1/state`, etc.).
- `url.scheme` — request scheme (`http`, `https`).

The middleware records into the global meter installed by `init_otel`. When OTel is disabled, the middleware records into the `opentelemetry` crate's no-op meter and the measurements never reach an exporter.

## Process collector

`spawn_process_collector(_cancel: CancellationToken) -> ProcessCollectorHandle` spawns the `opentelemetry-system-metrics` observer (`init_process_observer`) under a tokio task and returns a wrapper handle. The observer ticks on the standard `OTEL_METRIC_EXPORT_INTERVAL` interval (default 30 s, configurable via env), reads `sysinfo` snapshots of the current process, and records gauges against the global meter installed by `init_otel`. Emitted instruments (OTel dotted names; the OTLP→Prometheus collector translates dots to underscores so the scrape names are the `process_*` variants shown):

| OTel name | Scrape name | Type | Unit | Attributes |
|---|---|---|---|---|
| `process.cpu.usage` | `process_cpu_usage` | f64 gauge | percent | none |
| `process.cpu.utilization` | `process_cpu_utilization` | f64 gauge | percent | `process.pid`, `process.executable.name`, `process.executable.path`, `process.command` |
| `process.memory.usage` | `process_memory_usage` | i64 gauge | byte | same four `process.*` |
| `process.memory.virtual` | `process_memory_virtual` | i64 gauge | byte | same four `process.*` |
| `process.disk.io` | `process_disk_io` | i64 gauge | byte | same four `process.*` plus `direction=read\|write` |

This surface differs from the `metrics_process` exposition the prior recorder emitted (no `process_cpu_seconds_total`, `process_resident_memory_bytes`, `process_start_time_seconds`, `process_open_fds`, `process_max_fds`, or `process_threads`). Dashboards that filtered on those names need to be updated; ATC's bundled Grafana dashboard (`deploy/grafana/atc-postgres-overview.json`) only queries `atc_pg_*` and is unaffected. Operators relying on host- or container-level fd / start-time metrics should source them from the node exporter or container runtime sidecar instead.

The observer's `init_process_observer` loop runs forever — there is no cooperative shutdown surface on the upstream crate. `ProcessCollectorHandle::shutdown()` calls `tokio::task::AbortHandle::abort()` and returns the underlying `JoinHandle<()>` so the orchestration in `shutdown.rs` can await it under `SHUTDOWN_TIMEOUT_METRICS`. The observer does no DB/network work, so an abort between ticks is the common case and an abort mid-tick is safe.

## Operational metrics

All `atc_pg_*` metrics are emitted unlabeled per-process. Replica identity is added at ingest as standard target attributes (`pod`, `instance`) — the exact attachment mechanism depends on the collector configuration; the metrics themselves are agnostic. Cross-replica aggregation in alerts and dashboards uses `avg by (pod)`, `max by (pod)`, etc.

The blocks below are listed in roughly the order an event traverses the pipeline: webhook write → outbox row → NOTIFY emission → listener receipt → drain pass → broadcast → snapshot cursor → drain shutdown.

### `atc_pg_write_failures_total`

- **Name:** `atc_pg_write_failures_total`
- **Type:** counter
- **Attributes:** emitted `kind` ∈ `{parity, transient}`; injected `pod`, `instance`. `kind="parity"` fires when the PG UPSERT matches 0 rows (the WHERE predicate rejected the transition under PG's view of state); `kind="transient"` fires on sqlx errors at `pool.begin()`, mid-transaction, or `tx.commit()`. A `WARN` log with `target_status` is emitted alongside every parity rejection to surface which status the rejected transition was targeting (the `from` state is unavailable at the SQL layer — only `rows_affected` is returned).
- **Measures:** Webhook writes that failed inside `PgStore::apply_*_event`. Parity rejections return a 200 `{"status":"rejected"}` to GitHub and are NOT retried. Transient failures return 503 and ARE retried by GitHub's webhook delivery. Sustained nonzero rates of either kind indicate a real problem: parity means state-machine drift between PG and the in-memory model (page-worthy); transient means the database path is unhealthy.
- **Per-replica vs cluster:** Per-replica — only the writer replica increments. In multi-replica deployments any single replica can be the writer for a given webhook (GitHub picks one ingress).
- **Aggregation:** `sum by (kind)` cluster-wide for severity routing (parity → page; transient → alert on sustained rate). `max by (pod)` to localize a misbehaving replica.
- **Example PromQL:** `sum by (kind) (rate(atc_pg_write_failures_total[5m]))`

### `atc_pg_in_memory_drift_total`

- **Name:** `atc_pg_in_memory_drift_total`
- **Type:** counter
- **Attributes:** none emitted; `pod`, `instance` (injected).
- **Measures:** Events where the PG transaction committed successfully but the in-memory `RunStateMachine` apply on the same replica subsequently diverged. The committed PG row is durable and recoverable from the outbox, so a single increment is not data loss — but a sustained rate signals a code defect in the in-memory state machine and warrants a page.
- **Per-replica vs cluster:** Per-replica observation; cluster-relevant signal because any replica's drift indicates a logic bug independent of which one observed it.
- **Aggregation:** `sum without (pod, instance)` for cluster-wide drift rate; alert on any nonzero sustained rate.
- **Example PromQL:** `sum(rate(atc_pg_in_memory_drift_total[5m]))`

### `atc_pg_notify_emitted_total`

- **Name:** `atc_pg_notify_emitted_total`
- **Type:** counter
- **Attributes:** emitted `kind` ∈ `{run, job}` matching the event discriminator; injected `pod`, `instance`. Incremented by `PgStore::apply_*_event` after `tx.commit()` succeeds (the in-transaction `pg_notify` call is queued by PG and delivered on commit; aborted transactions silently drop it, so this counter only increments when the NOTIFY actually went out).
- **Measures:** Successfully committed write transactions broadcast to `LISTEN atc_outbox`. This is the writer-side "what was published" signal; the listener-side counterpart is `atc_pg_notify_received_total`.
- **Per-replica vs cluster:** Per-replica (only the writer replica increments for a given seq). Cluster-wide ingestion volume is the useful aggregation; per-replica view is rarely meaningful.
- **Aggregation:** `sum by (kind) (rate(...))` for cluster ingestion rate split by event kind. Use `sum without (pod, instance)` if you do not care about kind.
- **Example PromQL:** `sum by (kind) (rate(atc_pg_notify_emitted_total[5m]))`

### `atc_pg_notify_received_total`

- **Name:** `atc_pg_notify_received_total`
- **Type:** counter
- **Attributes:** none emitted; `pod`, `instance` (injected).
- **Measures:** NOTIFY payloads received by this replica's listener task on the `atc_outbox` channel. Every replica's listener receives every NOTIFY (PG fans out to all sessions holding `LISTEN atc_outbox`), so the per-replica rate should track parity across replicas. A replica whose rate falls behind the others has a stuck or stalled listener.
- **Per-replica vs cluster:** Per-replica.
- **Aggregation:** `avg by (pod) (rate(...))` to verify parity across replicas; `min by (pod) (rate(...))` to flag a replica whose listener is stuck. Sqlx hides successful reconnects, so a counter that briefly plateaus and then catches up is a normal reconnect; a counter that stops without resuming is a stuck listener.
- **Example PromQL:** `rate(atc_pg_notify_received_total[5m])`

### `atc_pg_listener_recv_errors_total`

- **Name:** `atc_pg_listener_recv_errors_total`
- **Type:** counter
- **Attributes:** none emitted; `pod`, `instance` (injected).
- **Measures:** Receive errors surfaced by the listener task (e.g., connection drops that sqlx could not silently reconnect through). Sqlx attempts to reconnect transparently on most listener errors; this counter only fires when the error escapes that retry loop. A nonzero rate over more than a single scrape window means the listener is repeatedly failing to recover.
- **Per-replica vs cluster:** Per-replica.
- **Aggregation:** `max by (pod) (rate(...))` — a single misbehaving replica is the actionable signal; sustained nonzero rate on any pod warrants investigation (likely DSN / session-mode misconfiguration; see `backend-server.md` § "DSN session-mode contract").
- **Example PromQL:** `rate(atc_pg_listener_recv_errors_total[5m])`

### `atc_pg_drain_passes_total`

- **Name:** `atc_pg_drain_passes_total`
- **Type:** counter
- **Attributes:** none emitted; `pod`, `instance` (injected). Heartbeat-only wakes (the 5-second readiness tick that fires `last_drain_pass_at` updates without doing any draining) do NOT increment — only NOTIFY-driven passes count.
- **Measures:** NOTIFY-driven drain passes completed by this replica. A flat-zero rate during a period of nonzero `atc_pg_notify_received_total` indicates the drain task is wedged.
- **Per-replica vs cluster:** Per-replica.
- **Aggregation:** `rate(... [5m]) by (pod)` — verify that drain passes are running on every replica that is receiving NOTIFYs. Pair with `atc_pg_notify_received_total` for a "wake → drain" sanity check.
- **Example PromQL:** `rate(atc_pg_drain_passes_total[5m])`

### `atc_pg_drain_rows_total`

- **Name:** `atc_pg_drain_rows_total`
- **Type:** counter
- **Attributes:** none emitted; `pod`, `instance` (injected).
- **Measures:** Outbox rows fetched and processed by the drain task across all paginated batches. Useful as a writer-vs-drain throughput sanity check: cluster-wide `rate(atc_pg_drain_rows_total)` summed across replicas should approximately equal `rate(atc_pg_notify_emitted_total)` × replica count over the same window (each replica's drain reads every committed row).
- **Per-replica vs cluster:** Per-replica.
- **Aggregation:** `sum by (pod)` per-replica; `sum without (pod, instance)` for cluster total.
- **Example PromQL:** `rate(atc_pg_drain_rows_total[5m])`

### `atc_pg_drain_duplicate_skipped_total`

- **Name:** `atc_pg_drain_duplicate_skipped_total`
- **Type:** counter
- **Attributes:** none emitted; `pod`, `instance` (injected).
- **Measures:** Outbox rows fetched during a drain pass but suppressed by the ring-buffer dedup because they had already been broadcast in a previous pass. Nonzero rate is the gap-healing rescan signal: the drain re-fetched a range of seqs because a NOTIFY arrived for a seq below the local watermark, and dedup correctly suppressed re-broadcast. Brief nonzero values during reorder windows are normal; a sustained high rate means the drain is repeatedly rescanning the same range and indicates either backstop math drift or an upstream NOTIFY storm.
- **Per-replica vs cluster:** Per-replica.
- **Aggregation:** `max by (pod) (rate(...))` — sustained nonzero rate on any single replica is the actionable signal.
- **Example PromQL:** `rate(atc_pg_drain_duplicate_skipped_total[5m])`

### `atc_pg_drain_unknown_kind_total`

- **Name:** `atc_pg_drain_unknown_kind_total`
- **Type:** counter
- **Attributes:** none emitted; `pod`, `instance` (injected).
- **Measures:** Outbox rows whose `kind` discriminator was neither `run` nor `job`. The set of legal kinds is fixed by a CHECK constraint on the outbox table, so this counter should be flat zero in any healthy deployment. A nonzero value is either a deploy-skew signal (an older replica writing a kind a newer replica does not understand, or vice versa) or a schema invariant violation; alert on first observation.
- **Per-replica vs cluster:** Per-replica observation; cluster-relevant signal.
- **Aggregation:** `sum without (pod, instance) (increase(...))` over a multi-hour window for the alert rule.
- **Example PromQL:** `increase(atc_pg_drain_unknown_kind_total[1h])`

### `atc_pg_outbox_lag_seconds`

- **Name:** `atc_pg_outbox_lag_seconds`
- **Type:** histogram (base-2 exponential aggregation; see [Histogram aggregation](#histogram-aggregation))
- **Attributes:** none emitted; `pod`, `instance` (injected)
- **Measures:** Event age at broadcast — `clock.now() - row.inserted_at` recorded once per broadcast row, where `clock` is `PgStore.clock: Arc<dyn Clock>`. The metric is more accurately "event age at broadcast" than "drain lag": `inserted_at DEFAULT now()` evaluates `transaction_timestamp()` (transaction start, not commit), so the metric includes writer-side transaction latency in addition to drain queueing. Operators reading p99/p95 should interpret it as "how stale is a typical row at broadcast time," not "how far behind is my drain task." Routing the now-side through `PgStore.clock` makes the observation deterministic under `TestClock` — see `tests/integration/pg_clock_seam_tests.rs::outbox_lag_is_deterministic_under_test_clock`.
- **Per-replica vs cluster:** Per-replica — each replica's drain task records its own observations from its own broadcasts.
- **Aggregation:** `histogram_quantile(0.99, sum(rate(...)) by (le, pod))` then `max by (pod)` for alerting — the slowest replica is the operationally relevant signal because all replicas serve traffic.
- **Example PromQL:** `histogram_quantile(0.99, sum(rate(atc_pg_outbox_lag_seconds_bucket[5m])) by (le, pod))`

### `atc_pg_drain_pass_duration_seconds`

- **Name:** `atc_pg_drain_pass_duration_seconds`
- **Type:** histogram (base-2 exponential aggregation)
- **Attributes:** none emitted; `pod`, `instance` (injected)
- **Measures:** Wall time from drain-pass start to drain-pass exit, including all paginated batches in the pass. NOT recorded for heartbeat-only wakes.
- **Per-replica vs cluster:** Per-replica — drain runs independently on each replica.
- **Aggregation:** `histogram_quantile(0.99, ...)` `by (pod)` for per-replica latency; `avg by (pod)` for trend tracking.
- **Example PromQL:** `histogram_quantile(0.99, sum(rate(atc_pg_drain_pass_duration_seconds_bucket[5m])) by (le, pod))`

### `atc_pg_wake_coalesced_total`

- **Name:** `atc_pg_wake_coalesced_total`
- **Type:** counter
- **Attributes:** none emitted; `pod`, `instance` (injected)
- **Measures:** NOTIFY arrivals observed by the listener while a drain pass was in flight (`drain_in_flight=true`). Counts arrival rate, NOT extra-pass rate (Tokio's `Notify` permit collapses N permits into 1 — the metric is about NOTIFY arrival vs drain-pass scheduling, which is what operators want).
- **Per-replica vs cluster:** Per-replica.
- **Aggregation:** `rate(... [5m]) by (pod)` then `max by (pod)` — sustained high values on any replica indicate a NOTIFY storm or slow drain.
- **Example PromQL:** `rate(atc_pg_wake_coalesced_total[5m])`

### `atc_pg_drain_startup_seconds`

- **Name:** `atc_pg_drain_startup_seconds`
- **Type:** histogram (base-2 exponential aggregation)
- **Attributes:** none emitted; `pod`, `instance` (injected)
- **Measures:** Startup readiness latency — wall time from `COALESCE(MAX(seq),0)` watermark init through first drain pass exit. One observation per process lifetime. Per the restart-recovery contract there is no historical replay; this measures startup readiness, NOT catch-up backlog.
- **Per-replica vs cluster:** Per-replica.
- **Aggregation:** `max by (pod)` over a window covering recent deploys (1h) — the slowest replica's startup is the operational signal.
- **Example PromQL:** `histogram_quantile(0.99, sum(rate(atc_pg_drain_startup_seconds_bucket[1h])) by (le, pod))`

### `atc_pg_drain_shutdown_remaining_rows`

- **Name:** `atc_pg_drain_shutdown_remaining_rows`
- **Type:** histogram (base-2 exponential aggregation)
- **Attributes:** none emitted; `pod`, `instance` (injected)
- **Measures:** Outbox rows whose `seq` is greater than this replica's drain watermark at drain task exit time. One observation per process lifetime, recorded after the drain loop exits on `cancel.cancelled()` and before the spawned task returns. Validates the cooperative-shutdown design: the drain task does NOT attempt to flush the outbox before exit — it completes the in-flight pass (if any) and stops, on the assumption that the unscanned tail rarely exceeds one drain pass (`DRAIN_BATCH_SIZE = 500`). Sustained observations above 500 should prompt either a drain-pass tuning review or a longer `terminationGracePeriodSeconds`. **Observation timing nuance:** the count is taken at drain task exit, not at signal arrival; the webhook handler keeps writing outbox rows until axum's graceful shutdown drains in-flight requests, so rows committed during that window are included. Operators reading this metric are seeing "what was unscanned when the drain task gave up," not "how far behind the drain was when SIGTERM arrived." When the post-shutdown count query fails or exceeds its 1-second timeout, the observation is skipped (logged as a warning) rather than recorded as zero, so `_count` only advances on successful observations.
- **Per-replica vs cluster:** Per-replica — each replica's drain task records its own observation against its own watermark.
- **Aggregation:** `histogram_quantile(0.99, ...)` `by (pod)` over a multi-deploy window (e.g. 24h) — the slowest replica's tail at shutdown is the actionable signal. `max by (pod) (rate(atc_pg_drain_shutdown_remaining_rows_count[24h]))` confirms each replica is recording observations across rollouts (a flat zero on a pod that recently restarted indicates the count query failed at shutdown — see warnings in the application log).
- **Example PromQL:** `histogram_quantile(0.99, sum(rate(atc_pg_drain_shutdown_remaining_rows_bucket[24h])) by (le, pod))`

### `atc_pg_broadcast_watermark`

- **Name:** `atc_pg_broadcast_watermark`
- **Type:** gauge
- **Attributes:** none emitted; `pod`, `instance` (injected)
- **Measures:** Highest outbox seq broadcast by this replica's drain task — the commit-order cursor read by `state_handler` as `lastSeq` in PG mode. Mirrors the per-replica `Arc<AtomicI64>` after each successful drain pass; seeded at startup from `COALESCE(MAX(seq),0)`.
- **Per-replica vs cluster:** Per-replica — each replica advances its watermark independently.
- **Aggregation:** Display per-pod (`atc_pg_broadcast_watermark`); for a single cluster-wide "laggiest replica" series, use `min(atc_pg_broadcast_watermark)` (or equivalently `min without (pod, instance)`). Note: `min by (pod) (atc_pg_broadcast_watermark)` would just preserve one series per pod — same as the per-pod display.
- **Example PromQL:** `atc_pg_broadcast_watermark`

### `atc_pg_min_pending_seq`

- **Name:** `atc_pg_min_pending_seq`
- **Type:** gauge
- **Attributes:** none emitted; `pod`, `instance` (injected)
- **Measures:** Lowest pending NOTIFY seq below the watermark (the gap-healing pressure signal). Mirrors the per-replica `min_pending_seq: Arc<AtomicI64>` after each listener `fetch_min`; reset to `f64::NAN` (the sentinel state) when the drain swaps the atomic to `i64::MAX` after catching up. NaN is preferred over `i64::MAX as f64` (≈ 9.22e18) because the float64 representation would push the y-axis of dashboards displaying watermark and min_pending_seq together to ~9e18, hiding the actual divergence signal at the watermark level.
- **Per-replica vs cluster:** Per-replica.
- **Aggregation:** Display per-pod alongside `atc_pg_broadcast_watermark`. Filter NaN with `... unless on() (atc_pg_min_pending_seq != atc_pg_min_pending_seq)` if needed.
- **Example PromQL:** `atc_pg_min_pending_seq` (Grafana renders NaN as gaps)

## Span inventory

Spans declared by ATC, grouped by the boundary they decorate.

### State snapshot path

| Span | Source | Attributes |
|---|---|---|
| `state.snapshot` | `backend/crates/atc-server/src/routes.rs` (`state_handler`) — root request span for `GET /v1/state`. Built manually (not via `#[instrument]`) so span fields can be recorded from the snapshot response before the handler returns. No `traceparent` extraction: `/v1/state` is a client-pull endpoint with no upstream trace context today. | `http.route="/v1/state"`, `snapshot.runs_count` (usize; late-bound, recorded after snapshot is read), `snapshot.jobs_count` (usize; late-bound), `snapshot.last_seq` (u64; late-bound). |
| `persist.read.snapshot` | `PgStore::read_snapshot` in `persist/pg.rs` and `InMemoryStore::read_snapshot` in `persist/in_memory.rs` — via `#[tracing::instrument]`, child of `state.snapshot`. | `last_seq` (u64; late-bound), `runs_count` (usize; late-bound), `jobs_count` (usize; late-bound). |

### Webhook ingestion path

| Span | Source | Attributes |
|---|---|---|
| `webhook.handler` | `backend/crates/atc-server/src/routes.rs` (`webhook_handler`) — root request span built in the handler body so `traceparent` extraction can attach the parent context before the span is entered. | `http.route="/v1/webhooks/github"`, `webhook.delivery_id` (recorded after parsing `x-github-delivery`), `webhook.event_type` (recorded after parsing `x-github-event`). The two `webhook.*` fields are declared as `tracing::field::Empty` at construction. |
| `webhook.verify` | `backend/crates/atc-github/src/webhook/verify.rs` (`verify_signature`) | `webhook.signature.present` (bool), `webhook.signature.algorithm="sha256"`. Secret, body bytes, and the signature value are explicitly skipped (`skip(secret, body, signature)`). |
| `webhook.parse` | `backend/crates/atc-github/src/webhook/mod.rs` (`parse_webhook`) | `webhook.event_type`, `webhook.action` (late-bound; recorded after the action is decoded). Body bytes are skipped. |

### Persist path

| Span | Source | Attributes |
|---|---|---|
| `persist.apply.run_event` | `PgStore::apply_run_event` in `persist/pg.rs` and `InMemoryStore::apply_run_event` in `persist/in_memory.rs` | `run_id` (i64); `seq` (i64; late-bound, recorded after the outbox row's `BIGSERIAL` is allocated). |
| `persist.apply.job_event` | `PgStore::apply_job_event` in `persist/pg.rs` and `InMemoryStore::apply_job_event` in `persist/in_memory.rs` | `run_id`, `job_id` (both i64); `seq` (late-bound for `PgStore`). |
| `persist.notify.emit` | `notify_outbox_seq_in_txn` in `persist/pg.rs` — wraps `SELECT pg_notify('atc_outbox', $1)` inside the `apply_*` transaction. | `notify.kind` (`"run"` / `"job"`), `notify.seq` (i64). |

Inner transaction helpers (`upsert_run_in_txn`, `upsert_job_in_txn`, `insert_outbox_run_in_txn`, `insert_outbox_job_in_txn`) carry default `#[tracing::instrument(skip_all)]` spans and inherit context from the surrounding `persist.apply.*` span.

### Listener path

| Span | Source | Attributes |
|---|---|---|
| `listener.task` | `backend/crates/atc-server/src/listener.rs` (`spawn_listener_task`, spawned from `PgStore::start_inner` per ADR-0006) — task-lifetime root span constructed at spawn time and attached to the spawned future via `.instrument(...)`. | none (long-lived). |
| `listener.recv` | `handle_notification` in `listener.rs` — per-NOTIFY child of `listener.task`. | `notify.payload.seq` (i64; the seq carried by the NOTIFY payload). |

### Drain path

| Span | Source | Attributes |
|---|---|---|
| `drain.task` | `backend/crates/atc-server/src/listener.rs` (`spawn_drain_task`, spawned from `PgStore::start_inner` per ADR-0006) — task-lifetime root span constructed at spawn time and attached via `.instrument(...)` so per-pass children attach to it instead of becoming fresh roots. | none (long-lived). |
| `drain.pass` | `drain_pass` in `listener.rs` — per-pass child. | `pass.start_floor` (i64), `pass.rows_fetched` (u64; recorded after pagination), `pass.batches` (u64; recorded after pagination). |
| `drain.broadcast` | constructed inside the per-row loop in `drain_pass`. | `seq` (i64), `kind` (`"run"` / `"job"`), `outbox_lag_ms` (i64). |

### Eviction path (in-memory mode only)

| Span | Source | Attributes |
|---|---|---|
| `eviction.sweep` | `InMemoryStore::evict_expired` in `persist/in_memory.rs` — per-tick root span. Spawned from `InMemoryStore::spawn_eviction`, which deliberately omits a task-lifetime parent (`.instrument(...)`): a long-lived root would never end until process shutdown, so each tick would attach to a span the SDK couldn't export until then. Per-tick roots mean every sweep exports as one tidy trace on tick. | `jobs.evicted` (u64; recorded after the sweep), `runs.evicted` (u64), `elapsed.micros` (u64). Recorded on both the eviction and the no-op-sweep code paths. |
