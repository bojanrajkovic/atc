# Observability — metric and span surface

Last verified: 2026-05-15

> **Note on issue #16 (runner-pool capacities).** The wire field
> `StateSnapshot.runner_pool_capacities` carries operator-declared capacity
> from `AppState` onto the snapshot response. It is **not** a metric and
> introduces no new `atc_runner_pool_*` instrument — ADR 0004 keeps pool
> stats derivation on the frontend. See `docs/architecture-decisions/0004-frontend-derived-pool-stats.md` and the
> design plan at `docs/design-plans/2026-05-13-issue-16-runner-pool-capacity.md`.

> **Note on the persistence crate split (ADR 0008).** The broadcast
> envelope type — referenced throughout this document as the value drained
> from the outbox and forwarded to WS subscribers — was renamed from
> `SeqEvent` to `CommittedEvent` and moved into the new `atc-wire` crate.
> No metric or span name changes. The trait that owns `subscribe()` and
> `shutdown()` now lives in the `atc-persist` crate (the `PgStore` /
> `InMemoryStore` impls stay in `atc-server` until the per-store crate
> extractions land). See `docs/architecture-decisions/0008-persistence-crate-split.md`.

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
- `listener.recv` — per-NOTIFY root span for the PG listener. Each notification exports as its own root trace.
- `drain.pass`, `drain.broadcast` — per-pass root and per-row child for the outbox drain. `drain.broadcast` is nested under `drain.pass` because `info_span!("drain.broadcast").in_scope(...)` runs inside the `drain_pass`-instrumented function.
- `eviction.sweep` — per-tick root span for the in-memory-mode TTL eviction sweep. Each `InMemoryStore::evict_expired` call is its own root, so every tick exports as one tidy trace.

Span names are stable identifiers — operators build dashboards and alerts that filter on them. Do not rename a span without coordinating with the dashboard owners; in particular, do not change `webhook.*`, `persist.*`, `listener.*`, or `drain.*` names without updating the doc here in lockstep.

### Span attribute conventions

- Lowercase, dotted keys (e.g., `webhook.delivery_id`, `pass.rows_fetched`). Use OpenTelemetry semantic conventions where they apply (`http.route`, `http.request.method`, `http.response.status_code`).
- Late-bound fields use `tracing::field::Empty` at span construction and `Span::current().record(...)` once the value is known. The `webhook.handler` span declares `webhook.delivery_id` and `webhook.event_type` as `Empty` and records them inside the handler body once the headers have been parsed.
- Never put webhook bodies, signatures, secrets, or full URLs (with secrets in query strings) on a span. The webhook attributes capture identifiers (`delivery_id`, `event_type`, `action`) and presence flags (`signature.present`), not payloads.

### Boundary discipline

New instrumentation goes at one of these boundaries:

- **API boundaries.** Every public HTTP route handler that performs work worth tracing (today: `webhook_handler`). The `axum-otel-metrics` middleware is the duration / status-code surface for *every* HTTP route automatically; per-route span instrumentation only needs to be added when the handler does enough work that the operator wants to see its internal structure.
- **Persist boundaries.** `PgStore::apply_*_event` and the in-transaction outbox / notify helpers under `atc-store-pg/src/store/writes.rs`. Internal SQL helpers nested inside an `apply_*` span inherit context via the default `#[instrument]` skip rules.
- **Background-task boundaries.** Long-lived futures spawned with `tokio::spawn` do NOT take a task-lifetime root span. Decorate the per-tick handler function (`listener.recv`, `drain.pass`, `eviction.sweep`) with `#[tracing::instrument(...)]` directly, so each iteration emits its own root that exports on completion. A wrapper at the spawn site is an anti-pattern — see [Task-lifetime root spans are an anti-pattern](#task-lifetime-root-spans-are-an-anti-pattern) below.

Do NOT decorate every internal function with `#[tracing::instrument]`. Spans are an operator surface; not an internal call graph. If a function is not load-bearing for an operator reading a flame graph, leave it uninstrumented and let it inherit the surrounding span.

### Cached instrument convention

Every repeat-emit metric in PG mode MUST go through a cached OTel `Counter` / `Histogram` instrument held on the `PgMetrics` struct in `backend/crates/atc-store-pg/src/metrics.rs`. `PgStore::start` calls `PgMetrics::register(...)` once after the global meter provider is installed; the resulting `Arc<PgMetrics>` is cloned into the listener and drain task closures, and every emit on a hot path is a field access (`metrics.drain_rows.add(N, &[])`) instead of building an instrument inline at every call.

For attribute-bearing instruments (`atc_pg_write_failures_total{kind=…}`, `atc_pg_notify_emitted_total{kind=…}`), `PgMetrics` stores **one instrument per metric name** plus pre-built `[KeyValue; N]` attribute slices alongside it (e.g. `attrs_parity: [KeyValue; 1]`). Emit sites read `counter.add(1, &self.attrs_parity)` so neither the instrument lookup nor the `KeyValue` allocation happens on a webhook path. Dedicated helpers (`write_failure_parity()`, `notify_emitted_run()`, etc.) wrap each `(instrument, slice)` pair so call sites never duplicate the `&self.attrs_*` reference.

Gauges use **`ObservableGauge<f64>`** instruments instead of sync `Gauge<f64>`. Each observable gauge's callback closes over an `Arc<AtomicI64>` (the same atomic the listener/drain already manipulate) and is invoked by the SDK on every collection cycle. The atomic update IS the metric update — production code never calls `record()` on these instruments. This avoids the delta-temporality footgun where a sync `Gauge` only surfaces on flushes that include a fresh `record()` call: an observable gauge re-reports its last-read value on every scrape, matching the semantics the OTel→Prometheus exporter expects for gauge-shaped metrics. The two PG-mode observable gauges (`atc_pg_broadcast_watermark`, `atc_pg_min_pending_seq`) take their atomics as parameters to `PgMetrics::register`; `register_build_info` registers an `atc_build_info` observable gauge whose callback always observes `1.0` with the compile-time label set.

This is **defense-in-depth, not micro-perf**. The hash-contract correctness fix from PR #153 was rooted in `metrics-util` / `metrics-exporter-otel`; with both crates retired in favor of direct OTel meters that bug class is gone at the SDK seam. The cached-instrument pattern still earns its keep by keeping hot-path emits allocation-free and making the metric surface a single grep-able struct.

**TODO(otel-0.32):** once `tracing-opentelemetry` and `axum-otel-metrics` publish releases targeting `opentelemetry 0.32` (upstream PRs `tokio-rs/tracing-opentelemetry#258` and `ttys3/axum-otel-metrics#196`), bump the SDK pin, enable the `experimental_metrics_bound_instruments` feature, and replace each `(instrument, [KeyValue; N])` pair with a real `BoundCounter<u64>` / `BoundHistogram<f64>` obtained via `Counter::bind(&[…])`. Emit-site shape stays identical; the wrapper helpers (`write_failure_parity` etc.) hide the swap.

Mechanical check (run before merging changes that touch backend sources):

```sh
rg -nU --multiline \
   'meter[^"]*"\)\s*\.(u64_counter|u64_observable_(?:counter|gauge|up_down_counter)|f64_(?:counter|gauge|histogram|observable_(?:counter|gauge|up_down_counter))|i64_(?:counter|gauge|histogram|observable_(?:counter|gauge|up_down_counter)))\([^)]+\)' \
   backend/crates/atc-server/src/ backend/crates/atc-store-pg/src/ \
   | rg -v 'crates/(atc-server/src/metrics|atc-store-pg/src/metrics)\.rs'
```

The grep should return no matches: the only sites that build OTel instruments live inside `atc-server::metrics` (`register_build_info`) and `atc-store-pg::metrics` (`PgMetrics::register_with_meter`). A new hit anywhere else is a reintroduced inline emit and must be moved onto `PgMetrics` (or, if it represents a genuinely new metric, added to `PgMetrics::register_with_meter` and documented under [Operational metrics](#operational-metrics)).

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

### Task-lifetime root spans are an anti-pattern

`tracing-opentelemetry` only exports a span to the OTel pipeline when it closes. A span attached to a `tokio::spawn`-ed future via `.instrument(span)` closes when the task ends — for long-lived background tasks (the listener loop, the drain loop, the eviction loop) that means "at process shutdown." Under normal operation the span never exports, and a SIGKILL or OOM kill loses it entirely along with every per-tick child that was waiting on it.

The fix is per-tick roots: decorate the handler function (`listener.recv`, `drain.pass`, `eviction.sweep`) with `#[tracing::instrument(name = "...", ...)]` directly. Each iteration is then a fresh root that exports as soon as it returns. `drain.broadcast` stays a child of `drain.pass` because it is constructed inside the instrumented `drain_pass` function via `info_span!("drain.broadcast").in_scope(...)`. The reference implementations are `backend/crates/atc-store-pg/src/listener.rs` and `backend/crates/atc-server/src/persist/in_memory.rs`. The rationale was first written down in `docs/design-plans/2026-05-13-eviction-fold-into-in-memory-store.md` (§ "Postscript") and extended to listener/drain in issue #170.

### Shutdown ordering

OTel SDK tear-down runs after every emitter has joined. The shutdown orchestration enumerates the ordering — see the comment block in `backend/crates/atc-server/src/shutdown.rs` (`run_shutdown_orchestration`, near the end). The principle is "no live emitter when shutdown fires." A new emitter category (a future scheduled job, a periodic task, a long-lived consumer) MUST be joined before the OTel shutdown step and named in that comment so the invariant continues to hold.

### Histogram aggregation

The meter provider registers an instrument view that maps every `Histogram` instrument to `Aggregation::Base2ExponentialHistogram { max_size, max_scale }`. The shared `exponential_histogram_view()` function in `backend/crates/atc-server/src/otel.rs` is the canonical hook for changing aggregation choice — both production `init_otel` and the test harness call it, so tests observe the same shape as production.

This requires the `spec_unstable_metrics_views` feature on `opentelemetry_sdk`. The feature is unstable per OTel's spec stability tracking — the API may shift on a future SDK release. The implementer who bumps `opentelemetry_sdk` owns reviewing the feature gate and, if it has been stabilized, removing the unstable flag.

The cross-format implication: when the OTLP collector translates an OTel exponential histogram into a Prometheus native histogram (Prometheus 2.40+), the resulting series does NOT emit `*_bucket` lines, so a classic-form query (`histogram_quantile(0.99, sum(rate(name_bucket[5m])) by (le, pod))`) returns empty. Against native histograms the equivalent is `histogram_quantile(0.99, sum(rate(name[5m])))` — no `_bucket` suffix, no `le` grouping — since `histogram_quantile` accepts native histograms as a single series, not a `le`-keyed bucket set. The OTel→Prometheus translator emits classic histograms by default; native-histogram emission requires explicit collector config. The bundled dashboard (`deploy/helm/atc/dashboards/atc-overview.json`) targets the classic form because that's what the default translator produces; operators running native-histogram-only collectors must translate panel queries accordingly. Dual-emission (classic AND native from the same source) is supported by some collector configurations and would let the classic-form query continue to work; ATC does not test against that configuration.

## atc_build_info

`register_build_info()` (called once at startup) sets a gauge always equal to `1.0` with these labels (emitted as OTel attributes; rendered as Prometheus labels by the collector):

| Label | Source | Example |
|---|---|---|
| `version` | `VERGEN_GIT_DESCRIBE` (via `build.rs`) | `v1.0.0` (mirrors `git_describe` — see below) |
| `git_describe` | `VERGEN_GIT_DESCRIBE` (via `build.rs`) | `v1.0.0` (exact tag for release-pipeline builds), `v1.0.0-3-gabc1234` (post-tag offset for local builds) |
| `git_sha` | `VERGEN_GIT_SHA` (via `build.rs`) | `a1b2c3d...` |
| `rustc_version` | `VERGEN_RUSTC_SEMVER` (via `build.rs`) | `1.94.0` |
| `build_timestamp` | `VERGEN_BUILD_TIMESTAMP` (via `build.rs`) | `2026-04-08T...` |
| `target_triple` | `VERGEN_CARGO_TARGET_TRIPLE` (via `build.rs`) | `x86_64-unknown-linux-gnu` |

`version` deliberately mirrors `git_describe` rather than carrying `CARGO_PKG_VERSION`. The operator-facing identifier should track the git tag the image was built from — which is also what `org.opencontainers.image.version` (the OCI label set by `docker/metadata-action`) and the `service.version` OTel resource attribute carry. Sourcing `version` from `Cargo.toml` instead lets the three identifiers drift apart on rc cycles where a tag is placed on a commit whose `Cargo.toml` was already bumped by release-please for the next stable release. The redundant column is left intact for any dashboards that already filter on `git_describe`.

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

| OTel name | Scrape name | Type | Unit | Value | Attributes |
|---|---|---|---|---|---|
| `process.cpu.usage` | `process_cpu_usage` | f64 gauge | percent | `raw_cpu_percent / core_count` (0..100% of host capacity) | `process.pid`, `process.executable.name`, `process.executable.path`, `process.command` |
| `process.cpu.utilization` | `process_cpu_utilization` | f64 gauge | percent | raw sysinfo `cpu_usage` summed across cores (0..N*100%) | none |
| `process.memory.usage` | `process_memory_usage` | i64 gauge | byte | resident memory | same four `process.*` |
| `process.memory.virtual` | `process_memory_virtual` | i64 gauge | byte | committed virtual memory | same four `process.*` |
| `process.disk.io` | `process_disk_io` | i64 gauge | byte | cumulative read/write bytes | same four `process.*` plus `direction=read\|write` |

The `process_cpu_usage` / `process_cpu_utilization` row inversion above is correct — `opentelemetry-system-metrics 0.31.0` binds the Rust variables to inverted constants (see crate source `src/lib.rs:131,214`): the Rust binding named `process_cpu_utilization` records `process_cpu_usage` (with attributes), and the binding named `process_cpu_usage` records `process_cpu_utilization` (no attributes). Dashboard queries that want per-process CPU (with pod attribution from the collector) should use `process_cpu_usage`, not `process_cpu_utilization`.

This surface differs from the `metrics_process` exposition the prior recorder emitted (no `process_cpu_seconds_total`, `process_resident_memory_bytes`, `process_start_time_seconds`, `process_open_fds`, `process_max_fds`, or `process_threads`). Dashboards that filtered on those names need to be updated; ATC's bundled Grafana dashboard (`deploy/helm/atc/dashboards/atc-overview.json`) covers the full `process_*`, `http_*`, `atc_pg_*`, `atc_config_*`, and `atc_build_info` surface. Operators relying on host- or container-level fd / start-time metrics should source them from the node exporter or container runtime sidecar instead.

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
- **Measures:** Highest outbox seq broadcast by this replica's drain task — the commit-order cursor read by `state_handler` as `lastSeq` in PG mode. Implemented as an OTel `ObservableGauge<f64>` whose callback reads the per-replica `broadcast_watermark: Arc<AtomicI64>` on every collection cycle; seeded at startup from `COALESCE(MAX(seq),0)` and advanced by the drain task after each successful pass. The atomic update IS the metric update — no separate `record()` call.
- **Per-replica vs cluster:** Per-replica — each replica advances its watermark independently.
- **Aggregation:** Display per-pod (`atc_pg_broadcast_watermark`); for a single cluster-wide "laggiest replica" series, use `min(atc_pg_broadcast_watermark)` (or equivalently `min without (pod, instance)`). Note: `min by (pod) (atc_pg_broadcast_watermark)` would just preserve one series per pod — same as the per-pod display.
- **Example PromQL:** `atc_pg_broadcast_watermark`

### `atc_pg_min_pending_seq`

- **Name:** `atc_pg_min_pending_seq`
- **Type:** gauge
- **Attributes:** none emitted; `pod`, `instance` (injected)
- **Measures:** Lowest pending NOTIFY seq below the watermark (the gap-healing pressure signal). Implemented as an OTel `ObservableGauge<f64>` whose callback reads the per-replica `min_pending_seq: Arc<AtomicI64>` and maps `i64::MAX` (the sentinel the drain swaps in once caught up) to `f64::NAN`; non-sentinel values pass through as-is. NaN is preferred over `i64::MAX as f64` (≈ 9.22e18) because the float64 representation would push the y-axis of dashboards displaying watermark and min_pending_seq together to ~9e18, hiding the actual divergence signal at the watermark level. The atomic update IS the metric update — no separate `record()` call.
- **Per-replica vs cluster:** Per-replica.
- **Aggregation:** Display per-pod alongside `atc_pg_broadcast_watermark`. Filter NaN with `... unless on() (atc_pg_min_pending_seq != atc_pg_min_pending_seq)` if needed.
- **Example PromQL:** `atc_pg_min_pending_seq` (Grafana renders NaN as gaps)

### `atc_pg_outbox_rows_deleted_total`

- **Name:** `atc_pg_outbox_rows_deleted_total`
- **Type:** counter
- **Attributes:** none emitted; `pod`, `instance` (injected)
- **Measures:** Outbox rows deleted by this replica's retention sweep task on each tick. Counted via the sweep statement's `DELETE ... RETURNING seq` row count, so the value reflects rows the replica actually deleted under `FOR UPDATE SKIP LOCKED` semantics — concurrent sweepers on other replicas account for disjoint candidate subsets, so the per-replica counter is the per-replica share, not a cluster-wide tally. Healthy at steady state: `rate(...)` ≈ outbox write rate divided by replica count. Sustained-zero rate after at least one full retention window indicates either the sweep predicate is rejecting everything (sub-floor retention misconfigured, no fresh heartbeats — see `atc_pg_outbox_min_replica_watermark`) or the outbox is not growing (no incoming webhooks).
- **Per-replica vs cluster:** Per-replica. Sum across replicas (`sum without (pod, instance)`) for total cluster-wide deletion rate.
- **Aggregation:** `sum without (pod, instance) (rate(atc_pg_outbox_rows_deleted_total[5m]))` for cluster-wide rate; `rate(atc_pg_outbox_rows_deleted_total[5m])` per pod to compare contention shares.
- **Example PromQL:** `sum without (pod, instance) (rate(atc_pg_outbox_rows_deleted_total[5m]))`

### `atc_pg_outbox_min_replica_watermark`

- **Name:** `atc_pg_outbox_min_replica_watermark`
- **Type:** gauge
- **Attributes:** none emitted; `pod`, `instance` (injected)
- **Measures:** `MIN(broadcast_watermark)` across non-stale replicas — the cluster-wide multi-replica safety floor that the sweep statement uses to bound deletions. Implemented as an OTel `ObservableGauge<f64>` whose callback reads the per-replica `min_replica_watermark_atomic: Arc<AtomicI64>` and maps `-1` to `f64::NAN`. `-1` is the NaN sentinel for two states: (a) the heartbeat task hasn't run yet (just-started replica), (b) no live replicas have heartbeated recently (cluster partition / shutdown). **Refreshed every 30 s by the outbox heartbeat task** — coarse-grained relative to OTel collection cadence; this is a cluster-state observation, not a per-event measurement.
- **Per-replica vs cluster:** Per-replica observation of a cluster-wide quantity. All replicas should observe the same value (within the 30 s heartbeat skew); divergence indicates one replica's heartbeat task is stalled.
- **Aggregation:** `min without (pod, instance) (atc_pg_outbox_min_replica_watermark)` for the cluster-wide signal; per-pod comparison surfaces stalled replicas.
- **Example PromQL:** `min without (pod, instance) (atc_pg_outbox_min_replica_watermark)` (Grafana renders NaN as gaps)

### `atc_config_reload_total`

- **Name:** `atc_config_reload_total`
- **Type:** counter
- **Attributes:** `result` (`"success"` | `"failure"`), `reason` (`"applied"` | `"noop"` | `"read"` | `"parse"` | `"validate"`); `pod`, `instance` (injected)
- **Measures:** Config-watcher reload attempts, labeled by outcome. `result="success",reason="applied"` — reload changed AppState and broadcast `ConfigUpdate`. `result="success",reason="noop"` — reload re-read the file but content matched current AppState (no broadcast). `result="failure",reason="read"` — file I/O failure (deleted file, permissions). `result="failure",reason="parse"` — YAML deserialization failure. `result="failure",reason="validate"` — zero capacity / empty labels / duplicate pool. Implemented as a sync `Counter<u64>` with pre-built `[KeyValue; 2]` attribute slices per outcome (cached-instrument convention) — call sites incur no allocation on emit.
- **Per-replica vs cluster:** Per-replica — each replica's watcher reloads independently from its local view of the ConfigMap mount.
- **Aggregation:** `sum without (pod, instance) (rate(atc_config_reload_total[5m]))` for cluster-wide reload rate; per-reason breakdown surfaces failure spikes. A sustained non-zero `reason="failure"` rate indicates the operator's most-recent YAML edit is invalid and the cluster is running on the previous good config.
- **Example PromQL:** `sum by (reason) (rate(atc_config_reload_total[5m]))`

### `atc_config_runner_pools`

- **Name:** `atc_config_runner_pools`
- **Type:** gauge
- **Attributes:** none emitted; `pod`, `instance` (injected)
- **Measures:** Number of operator-declared runner pools currently loaded in `AppState.runner_pool_capacities`. Reflects the startup-loaded count until the first applied reload, then tracks the latest applied reload's pool count. Implemented as an OTel `ObservableGauge<f64>` whose callback reads from `Arc<AtomicI64>` on every collection cycle (cached-instrument convention; the atomic update IS the metric update). `Weak<AtomicI64>` registration ensures dropped watchers do not leak callbacks.
- **Per-replica vs cluster:** Per-replica observation of a cluster-wide quantity. All replicas mount the same ConfigMap so values should match within the kubelet sync window; divergence (~60 s skew) is normal during a rolling ConfigMap update.
- **Aggregation:** `max without (pod, instance) (atc_config_runner_pools)` for the cluster-wide pool count; per-pod divergence during a rolling reload is expected.
- **Example PromQL:** `max without (pod, instance) (atc_config_runner_pools)`

### `atc_pg_outbox_oldest_row_age_seconds`

- **Name:** `atc_pg_outbox_oldest_row_age_seconds`
- **Type:** gauge
- **Attributes:** none emitted; `pod`, `instance` (injected)
- **Measures:** Age in seconds of the oldest outbox row, computed Rust-side as `clock.now() - MIN(inserted_at)`. Implemented as an OTel `ObservableGauge<f64>` whose callback reads the per-replica `oldest_row_age_seconds_atomic: Arc<AtomicI64>` and maps `-1` to `f64::NAN` (the empty-outbox sentinel). **Refreshed every 30 s by the outbox heartbeat task** — coarse-grained. Useful as the retention-headroom signal: under healthy steady state the value oscillates near `outbox_retention` and rate-of-change matches the sweep rate. A monotonically rising value past `outbox_retention` indicates the sweep is not deleting (verify `atc_pg_outbox_rows_deleted_total` rate; check for sub-floor retention or absent heartbeats).
- **Per-replica vs cluster:** Per-replica observation of a cluster-wide quantity (the outbox is shared). All replicas observe the same value within the heartbeat skew.
- **Aggregation:** `max without (pod, instance) (atc_pg_outbox_oldest_row_age_seconds)` for the cluster-wide signal.
- **Example PromQL:** `max without (pod, instance) (atc_pg_outbox_oldest_row_age_seconds)` (Grafana renders NaN as gaps)

## Span inventory

Spans declared by ATC, grouped by the boundary they decorate.

### State snapshot path

| Span | Source | Attributes |
|---|---|---|
| `state.snapshot` | `backend/crates/atc-server/src/routes.rs` (`state_handler`) — root request span for `GET /v1/state`. Built manually (not via `#[instrument]`) so span fields can be recorded from the snapshot response before the handler returns. No `traceparent` extraction: `/v1/state` is a client-pull endpoint with no upstream trace context today. | `http.route="/v1/state"`, `snapshot.runs_count` (usize; late-bound, recorded after snapshot is read), `snapshot.jobs_count` (usize; late-bound), `snapshot.last_seq` (u64; late-bound). |
| `persist.read.snapshot` | `PgStore::read_snapshot` in `atc-store-pg/src/store/writes.rs` and `InMemoryStore::read_snapshot` in `persist/in_memory.rs` — via `#[tracing::instrument]`, child of `state.snapshot`. | `last_seq` (u64; late-bound), `runs_count` (usize; late-bound), `jobs_count` (usize; late-bound). |

### Webhook ingestion path

| Span | Source | Attributes |
|---|---|---|
| `webhook.handler` | `backend/crates/atc-server/src/routes.rs` (`webhook_handler`) — root request span built in the handler body so `traceparent` extraction can attach the parent context before the span is entered. | `http.route="/v1/webhooks/github"`, `webhook.delivery_id` (recorded after parsing `x-github-delivery`), `webhook.event_type` (recorded after parsing `x-github-event`). The two `webhook.*` fields are declared as `tracing::field::Empty` at construction. |
| `webhook.verify` | `backend/crates/atc-github/src/webhook/verify.rs` (`verify_signature`) | `webhook.signature.present` (bool), `webhook.signature.algorithm="sha256"`. Secret, body bytes, and the signature value are explicitly skipped (`skip(secret, body, signature)`). |
| `webhook.parse` | `backend/crates/atc-github/src/webhook/mod.rs` (`parse_webhook`) | `webhook.event_type`, `webhook.action` (late-bound; recorded after the action is decoded). Body bytes are skipped. |

### Persist path

| Span | Source | Attributes |
|---|---|---|
| `persist.apply.run_event` | `PgStore::apply_run_event` in `atc-store-pg/src/store/writes.rs` and `InMemoryStore::apply_run_event` in `persist/in_memory.rs` | `run_id` (i64); `seq` (i64; late-bound, recorded after the outbox row's `BIGSERIAL` is allocated). |
| `persist.apply.job_event` | `PgStore::apply_job_event` in `atc-store-pg/src/store/writes.rs` and `InMemoryStore::apply_job_event` in `persist/in_memory.rs` | `run_id`, `job_id` (both i64); `seq` (late-bound for `PgStore`). |
| `persist.notify.emit` | `notify_outbox_seq_in_txn` in `atc-store-pg/src/store/writes.rs` — wraps `SELECT pg_notify('atc_outbox', $1)` inside the `apply_*` transaction. | `notify.kind` (`"run"` / `"job"`), `notify.seq` (i64). |

Inner transaction helpers (`upsert_run_in_txn`, `upsert_job_in_txn`, `insert_outbox_run_in_txn`, `insert_outbox_job_in_txn`) carry default `#[tracing::instrument(skip_all)]` spans and inherit context from the surrounding `persist.apply.*` span.

### Listener path

| Span | Source | Attributes |
|---|---|---|
| `listener.recv` | `handle_listener_notification` in `listener.rs` — per-NOTIFY root span. The spawn site (`spawn_listener_task`, spawned from `PgStore::start_inner` per ADR-0006) carries no task-lifetime wrapper; each notification's handler invocation emits its own root. | `notify.payload.seq` (i64; the seq carried by the NOTIFY payload). |

### Drain path

| Span | Source | Attributes |
|---|---|---|
| `drain.pass` | `drain_pass` in `listener.rs` — per-pass root span. The spawn site (`spawn_drain_task`, spawned from `PgStore::start_inner` per ADR-0006) carries no task-lifetime wrapper; each invocation of `drain_pass` emits its own root. | `pass.start_floor` (i64), `pass.rows_fetched` (u64; recorded after pagination), `pass.batches` (u64; recorded after pagination). |
| `drain.broadcast` | constructed inside the per-row loop in `drain_pass`, nested under `drain.pass` via `broadcast_span.in_scope(...)`. | `seq` (i64), `kind` (`"run"` / `"job"`), `outbox_lag_ms` (i64). |

### Eviction path (in-memory mode only)

| Span | Source | Attributes |
|---|---|---|
| `eviction.sweep` | `InMemoryStore::evict_expired` in `persist/in_memory.rs` — per-tick root span. Spawned from `InMemoryStore::spawn_eviction`, which deliberately omits a task-lifetime parent (`.instrument(...)`): a long-lived root would never end until process shutdown, so each tick would attach to a span the SDK couldn't export until then. Per-tick roots mean every sweep exports as one tidy trace on tick. | `jobs.evicted` (u64; recorded after the sweep), `runs.evicted` (u64), `elapsed.micros` (u64). Recorded on both the eviction and the no-op-sweep code paths. |

### Outbox retention path (PG mode only)

| Span | Source | Attributes |
|---|---|---|
| `outbox.heartbeat.tick` | `outbox_heartbeat_tick` in `atc-store-pg/src/store/retention.rs` — per-tick root span. Spawned from `spawn_outbox_heartbeat` (called from `PgStore::start_inner`), which deliberately omits a task-lifetime parent: a long-lived root would never end until process shutdown. Per-tick root means every heartbeat exports as one tidy trace. | `replica_id` (string; the `<hostname>-<uuid8>` identity bound to this `PgStore`), `broadcast_watermark` (i64; late-bound, the value upserted into `outbox_watermarks` this tick), `min_replica_watermark` (i64; late-bound, cluster-wide floor observed this tick — `-1` when no live replicas), `oldest_row_age_seconds` (i64; late-bound — `-1` when outbox is empty). |
| `outbox.sweep.tick` | `outbox_sweep_tick` in `atc-store-pg/src/store/retention.rs` — per-tick root span. Spawned from `spawn_outbox_sweep` (called from `PgStore::start_inner`), same no-task-lifetime-parent pattern. | `retention_seconds` (u64; the configured retention age), `rows_deleted` (u64; late-bound, count of outbox rows this sweep tick deleted under `FOR UPDATE SKIP LOCKED`), `watermarks_cleaned` (u64; late-bound, count of dead `outbox_watermarks` rows piggyback-cleaned in this tick). |
