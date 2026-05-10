# Issue #59 — OpenTelemetry Instrumentation (Tracing + Metrics)

> **Implementation Guidance:** Before writing any code for this plan, read [`docs/implementation-guidance.md`](../implementation-guidance.md). That document — not this plan — governs how implementation is executed (TDD discipline, branch/PR conventions, doc-mapping updates, ADR annotation sweeps, generated-file rules).

**PR title (squash-merged commit subject):** `feat(server): introduce OpenTelemetry instrumentation (tracing and metrics)`

## Context

Phase 5 metrics shipped (`docs/design-plans/2026-05-06-phase-5-operational-metrics.md`): six operational metrics + the `atc_pg_drain_shutdown_remaining_rows` follow-up landed via PR #84 (commit `a37f741`), all backed by `metrics-exporter-prometheus` 0.18 + `axum-prometheus` 0.10 with the global `metrics` facade. The runtime is correct and observable; what's missing is **distributed** observability — request-shape and trace-context — and a single emission path that downstream operators can plug into without per-tool config.

Today the backend uses `tracing_subscriber::fmt()` JSON or pretty (`main.rs:89–103`) for structured **logging**, not tracing. `atc-github` already carries `#[tracing::instrument]` on `parse_webhook` (`webhook/mod.rs:93`) and `verify_signature` (`webhook/verify.rs:57`), and `webhook_handler` carries one too (`routes.rs:229`). What's missing is the surrounding span hierarchy, the W3C Trace Context propagation, and an OTLP exporter to take these spans somewhere actionable. Without that, the existing `#[instrument]` annotations only produce span events in the JSON log stream — useful but not connected.

This issue replaces the local Prometheus emission path with the OTel SDK + OTLP export AND adds distributed tracing. Both flow through one OTel pipeline, one collector dependency, and one set of semantic conventions. The `/metrics` text-format endpoint goes away — operators run a collector (the author runs Grafana Alloy in his homelab) that ingests OTLP and re-exposes Prometheus-compatible scrape endpoints if needed.

The OTel Rust ecosystem version landscape (May 2026) splits across two trains: `opentelemetry`/`opentelemetry_sdk` 0.31 (current), `tracing-opentelemetry` 0.32.1 (supports both `opentelemetry` 0.31 and 0.32 — the intentional one-version lag of earlier releases no longer applies as of 0.32.1), and the HTTP middleware crates split: `tower-otel-http-metrics` is on `opentelemetry ^0.30.0`, `axum-otel-metrics` is on `^0.31`. The plan locks the matrix in Locked Decisions; version selection must be deliberate — resolved versions must be verified before the first compile.

## Definition of Done

1. **Distributed tracing live.** A webhook POST against an instance with `OTEL_EXPORTER_OTLP_ENDPOINT` set produces an OTLP trace at the configured endpoint with the expected span hierarchy: `webhook.handler` → {`webhook.verify`, `webhook.parse`, `persist.apply.run_event` (or `apply.job_event`) → `persist.notify.emit`} on the request side, plus `drain.task` → `drain.pass` → N×`drain.broadcast` on the drain side. Standard `service.name=atc` and `service.version` resource attributes set. W3C `traceparent` from incoming requests is extracted and used as the parent context.
2. **OTel-emitted metrics.** Every `atc_*` metric (eight counters, two gauges, four histograms, plus `atc_build_info`) emits through OTel via `metrics-exporter-otel`. The `metrics::counter!()`/`gauge!()`/`histogram!()` syntax is preserved at every emit site (zero call-site churn). Histograms emit as OTel **exponential** aggregations via the `spec_unstable_metrics_views` feature (locked below); the OTLP→Prometheus path produces native histograms in Prometheus 2.40+/Mimir/GrafanaCloud Prometheus.
3. **HTTP duration via OTel HTTP middleware.** `axum-prometheus`'s `PrometheusMetricLayer` is removed; replaced with **`axum-otel-metrics`** (locked below). Request duration emits with HTTP semantic-conventions attributes (`http.request.method`, `http.response.status_code`, `http.route`).
4. **`/metrics` endpoint removed; chart surface migrated.** atc-server no longer exposes `/metrics`. The metrics-side listener, the `ATC_METRICS_ADDR` config field, the chart's `metrics.*` values block, `config.metricsAddr`, the `metrics` service port, the ServiceMonitor template, and the `values.schema.json` entries that bind those keys are all removed. `deploy/helm/atc/tests/values-metrics.yaml` is removed (it asserts state that no longer exists). The chart's `README.md`, the `deploy/helm/atc/CLAUDE.md`, and `docs/architecture/ci-pipeline.md` (where it references `/metrics` as part of the operator surface) are updated. The Phase 5 custom-bucket overrides (`set_buckets_for_metric` in `metrics.rs:65–79`) are gone — exponential histograms self-resolve. **Breaking change:** chart consumers overriding `metrics.*` or `config.metricsAddr` will see render failures because `values.schema.json` has `additionalProperties: false`; signal in the chart's release notes / CHANGELOG.
5. **Default-disabled OTel posture, no SDK overhead.** With `OTEL_EXPORTER_OTLP_ENDPOINT` unset, no OTel SDK / provider / exporter / background task is initialized. `tracing_subscriber::fmt()` continues to log to stderr unchanged. `metrics::counter!()` macros resolve to the `metrics` crate's no-op recorder — the call cost is very low (one global-table check, no allocation, no I/O), but not literally zero. Use the precise framing "no SDK / no provider / no exporter / no background-task overhead," not "zero overhead."
6. **Helm chart `otel.*` values block.** When `otel.enabled: true`, deployment template injects the spec-standard `OTEL_*` env vars. ServiceMonitor template is removed (operators scrape the collector, not atc-server). `values.schema.json` adds the `otel.*` properties. New `tests/values-otel.yaml` fixture asserts the rendered surface; `tests/values-metrics.yaml` is deleted.
7. **Dedicated `just otel-dev-stack` recipe.** A new just recipe starts a Grafana **otel-lgtm** all-in-one container (single image bundling OTel collector + Loki + Tempo + Mimir + Grafana UI — gives traces, metrics, and logs visible in a Grafana frontend without standing up the full LGTM stack manually). `just dev` is **not** changed: backend + frontend dev servers continue to start without the observability stack. Operators / contributors who want to inspect OTel emissions run `just otel-dev-stack` in a separate terminal, then export `OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318` in the shell where they run `just dev`. The chart's deployment story still assumes Alloy (production), but the *dev* stack picks `otel-lgtm` because Alloy alone has no trace explorer and dev productivity benefits from a UI for traces + metrics + logs in one place.
8. **Cooperative shutdown integration.** `tracer_provider.shutdown()` and `meter_provider.shutdown()` plug into the existing `shutdown::run_shutdown_orchestration` (PR #81) so in-flight spans/metrics flush before process exit. Skipped cleanly when OTel was not initialized.
9. **Tests migrated.** Every test that imports `render_metrics`/`PROMETHEUS_INIT`/`ATC_METRICS_ADDR` from `tests/integration/common/mod.rs` is updated. As of HEAD this is **20+ files**: the 8 Phase 5 metrics tests already named, plus `metrics.rs` (the legacy /metrics HTTP-side-port test, deleted not migrated), `metrics_router_isolation.rs`, `config_tests.rs` (drops `metrics_addr` coverage), `db_readyz_tests.rs`, `e2e_tests.rs`, `gap_healing.rs`, `graceful_shutdown.rs`, `notify_listener_tests.rs`, `outbox_tests.rs`, `readyz.rs`, `routes_tests.rs`, `row_lock_serialization.rs`, `state_tests.rs`, `transactional_writes_tests.rs`, `webhook_ingestion_tests.rs`, `ws_tests.rs`. The implementing subagent runs the actual `grep -l 'render_metrics\|PROMETHEUS_INIT\|metrics_addr\|ATC_METRICS_ADDR' backend/` and migrates every hit. New `tracing_webhook_spans_test.rs` asserts span hierarchy + W3C propagation. New `otel_init_test.rs` covers default-disabled posture. All 327+ existing backend tests continue to pass (modulo the migrated metrics tests and the deleted `/metrics` HTTP-side-port test).
10. **Documentation sweep.** `docs/architecture/metrics.md` (the **canonical** doc-contract home for the metric authoring contract per `scripts/doc-mapping.sh:28`) is rewritten for OTLP semantics and extended to cover the new span authoring contract — the contract is **extended in place**, not relocated to `backend-server.md`. `docs/architecture/backend-server.md` gets a new § Tracing section that links to the metrics doc for the contract. `CONTRIBUTING.md` § Metrics renamed → § Observability cross-links to `metrics.md`. `scripts/doc-mapping.sh`, the two domain-level `CLAUDE.md` files (atc-server, helm chart), `docs/architecture/deployment.md`, `docs/architecture/ci-pipeline.md`, the chart's `README.md`, and any other doc referencing `/metrics` or ServiceMonitor as the operator-surface get updated.
11. **Manual smoke test.** Pod deployed to author's homelab with OTLP endpoint pointing at Alloy: traces visible in Alloy UI / downstream Tempo, all metrics visible at the collector's Prometheus exposition.

## Locked Decisions

Not open for re-evaluation during implementation:

- **Spec-standard `OTEL_*` env vars.** ATC reads `OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_SERVICE_NAME`, `OTEL_RESOURCE_ATTRIBUTES`, `OTEL_TRACES_SAMPLER`, `OTEL_TRACES_SAMPLER_ARG`, `OTEL_EXPORTER_OTLP_PROTOCOL` directly. OtelConfig is **not** modeled in figment Config — the SDK auto-reads the spec env vars where it can; we read them manually only where the SDK doesn't (and document any gaps).
- **Single PR for the whole migration.** Tracing + metrics swap + `/metrics` removal land together. Validation path: author's homelab Grafana Alloy + `kubectl` default context.
- **`/metrics` endpoint removed.** Operators run a collector (Alloy / vanilla otel-collector / contrib distribution) that exposes `/metrics` for Prometheus scrape if needed.
- **OTLP transport: HTTP/protobuf, only.** The default in `opentelemetry-otlp` 0.31.x; matches OTel ecosystem direction (gRPC dropped from defaults). The chart values block does **not** expose a `protocol` choice in this issue — gRPC support is explicitly out of scope. If a downstream operator needs gRPC later, that's a follow-up issue with its own `opentelemetry-otlp` feature-flag work.
- **`metrics-exporter-otel` drop-in.** Keep `metrics::counter!()`/`gauge!()`/`histogram!()` macros at every existing emit site; swap only the global recorder. The recorder is constructed via `OpenTelemetryRecorder::new(meter)` — note `meter`, not `MeterProvider`; the implementer obtains the meter via `meter_provider.meter("atc")`.
- **HTTP middleware: `axum-otel-metrics`.** Locked to the crate currently on the `opentelemetry/opentelemetry_sdk ^0.31` train (matches the rest of our OTel deps). `tower-otel-http-metrics` is rejected — it lags on `^0.30` and would force a contradictory version matrix. Eliminates the two-recorder hazard.
- **OTel ecosystem version train: `opentelemetry`/`opentelemetry_sdk` 0.31.x line** — verified at implementation time via `cargo add --dry-run`. `tracing-opentelemetry 0.32.1` supports both `opentelemetry` 0.31 and 0.32 (the one-version lag no longer applies as of this release). `metrics-exporter-otel` and `axum-otel-metrics` must both resolve to crates compatible with `opentelemetry_sdk` 0.31. Rule 3 (never pin) still applies — but the Rule-3 prose is updated in this plan to require `cargo add --dry-run` verification of the resolved matrix before the first compile, NOT to defer matrix selection to the resolver.
- **Exponential histogram views require `spec_unstable_metrics_views`.** Forcing OTel histograms into exponential aggregation requires `Stream::builder().with_aggregation(Aggregation::Base2ExponentialHistogram { ... })`, which is gated behind the `spec_unstable_metrics_views` feature on `opentelemetry_sdk`. **The plan locks this feature ON.** This is the trade-off for the native-histogram promise in DoD #2: the feature is unstable per OTel's spec-stability tracking, so the API may shift in a future SDK release. The implementer owns the upgrade-time review when bumping `opentelemetry_sdk`.
- **Default sampler: `ParentBased(root=AlwaysOn)`.** ATC is low-volume (~100 req/s peak, per existing capacity rationale). Operators tune via `OTEL_TRACES_SAMPLER`/`OTEL_TRACES_SAMPLER_ARG` if needed. The Rust SDK's pickup of `OTEL_TRACES_SAMPLER` is incomplete as of 0.31 (open upstream issue). **Operational fallback:** `init_otel()` reads `OTEL_TRACES_SAMPLER` directly via `std::env::var`. Accepted values: `always_on`, `always_off`, `traceidratio`, `parentbased_always_on` (default), `parentbased_always_off`, `parentbased_traceidratio`. `OTEL_TRACES_SAMPLER_ARG` parsed as `f64` for ratio variants; rejected (non-fatal: `tracing::warn!` + fall back to default) for non-numeric or out-of-range values. Invalid `OTEL_TRACES_SAMPLER` warns and falls back to the default — does not abort startup.
- **Conditional SDK install.** OTel SDK + tracer/meter providers initialized only when `OTEL_EXPORTER_OTLP_ENDPOINT` is set. Otherwise the SDK is never loaded — no localhost:4318 fallback (the SDK's default is to try; we override by simply not initializing).
- **W3C Trace Context propagator.** `TraceContextPropagator` registered globally; `traceparent` extracted from incoming HTTP requests via `opentelemetry-http::HeaderExtractor`. If the header is absent, the request span is a root span.
- **Resource attributes.** `service.name=atc` (default; overridable via `OTEL_SERVICE_NAME` — the default is injected only when `OTEL_SERVICE_NAME` is unset, i.e. gated on `env::var_os("OTEL_SERVICE_NAME").is_none()`; otherwise `Resource::builder().with_attributes` would clobber the operator's env-var override), `service.version=env!("CARGO_PKG_VERSION")` (the same string `atc_build_info` already uses at `metrics.rs:140` — synced to git tags by release-please), plus a non-spec `atc.git_sha=env!("VERGEN_GIT_SHA")` for build-traceability inside individual versions, plus anything from `OTEL_RESOURCE_ATTRIBUTES`. The build script (`build.rs`) already enables `vergen-gix` with `build`, `cargo`, `si`, `rustc` features — no build.rs change needed.
- **OTel SDK shutdown ordering** (extends PR #81 cooperative shutdown). After **all span/metric emitters have joined** — drain task, listener task, process collector (`metrics::spawn_process_collector`), and the HTTP middleware's request handler drain (axum-otel-metrics flushes via the meter provider, so its emissions go in before provider shutdown) — call `tracer_provider.shutdown()` → `meter_provider.shutdown()` → process exit. The principle is "no live emitter when shutdown fires." Document this explicitly in `shutdown.rs` with a comment naming each emitter the order depends on.
- **`#[tracing::instrument]` is the boundary tool.** Spans are created via `#[instrument]` attribute or explicit `tracing::info_span!()` at boundaries. **Tokio `spawn` does NOT propagate parent spans automatically** — futures spawned to background tasks (`spawn_drain_task`, `spawn_listener_task`) MUST be wrapped with `.instrument(span)` (per `tracing::Instrument` trait). This is a known gotcha; see internet research, "Recent breaking changes" section.
- **Phase 5 metric inventory unchanged.** All six Phase 5 metrics + `atc_pg_drain_shutdown_remaining_rows` continue to exist; they emit through the OTel pipeline. Their semantics, names, and attributes are preserved. (Histogram bucket configuration is what changes — exponential vs explicit.)
- **Each phase is a self-contained subagent dispatch.** Dispatching subagents per phase preserves main context. Each phase begins with TDD red, ends with green + lints/tests passing.

## Architecture

### A1 — OTel SDK initialization (`backend/crates/atc-server/src/otel.rs`, new module)

`init_otel(cfg: &Config) -> Option<OtelHandles>` returns `None` if `OTEL_EXPORTER_OTLP_ENDPOINT` is unset, `Some(handles)` otherwise. `OtelHandles` carries the **provider handles only** — `tracer_provider: SdkTracerProvider`, `meter_provider: SdkMeterProvider`, and the chosen tracer (`tracer: opentelemetry_sdk::trace::Tracer`). `main.rs` constructs the `OpenTelemetryLayer<S, T>` itself by calling `tracing_opentelemetry::layer().with_tracer(handles.tracer.clone())`. This avoids exporting a generic-typed layer through the module boundary, where the type parameter on the layer (`<S>` over the subscriber, `<T>` over the tracer) would need to be propagated.

When endpoint is set, the function:
1. Builds an OTLP HTTP/protobuf exporter pointed at the configured endpoint.
2. Reads `OTEL_TRACES_SAMPLER` and `OTEL_TRACES_SAMPLER_ARG` per the operational-fallback rules in Locked Decisions; constructs `Sampler` accordingly.
3. Builds `SdkTracerProvider` with the chosen sampler; obtains a `Tracer` via `tracer_provider.tracer("atc")`.
4. Builds `SdkMeterProvider` with `PeriodicReader` + OTLP exporter, registering an instrument view that maps every `Histogram` instrument to `Aggregation::Base2ExponentialHistogram { max_size, max_scale }` via `Stream::builder().with_aggregation(...)`. **Requires the `spec_unstable_metrics_views` feature on `opentelemetry_sdk`** — locked above.
5. Sets resource attributes: `service.name` defaults to `"atc"` but is injected only when `OTEL_SERVICE_NAME` is absent (`env::var_os("OTEL_SERVICE_NAME").is_none()`), so operator overrides via env are respected. Also sets `service.version` from `env!("CARGO_PKG_VERSION")` and `atc.git_sha` from `env!("VERGEN_GIT_SHA")`, plus anything the SDK auto-extracts from `OTEL_RESOURCE_ATTRIBUTES`.
6. Sets globals: `global::set_tracer_provider`, `global::set_meter_provider`, `global::set_text_map_propagator(TraceContextPropagator::new())`.
7. Installs `metrics-exporter-otel::OpenTelemetryRecorder` via `metrics::set_global_recorder()`.
8. Returns the `OtelHandles` so `main.rs` composes `tracing_opentelemetry::layer().with_tracer(handles.tracer.clone())` into the existing subscriber.

### A2 — `main.rs` subscriber composition

After current subscriber init (`main.rs:89–103`), branch on `init_otel()`:
- If `Some(handles)`: replace the existing `tracing_subscriber::fmt()...init()` with a registry-based composition that includes the env filter + fmt layer + OpenTelemetryLayer.
- If `None`: keep the existing fmt-only init unchanged. (The current `.init()` calls become a more compositional `tracing_subscriber::registry().with(fmt_layer).with(env_filter).init()` but functionally equivalent.)

The OTel handles flow into AppState (or are kept in a process-scope `Arc`) so the shutdown orchestration can call `.shutdown()` on them.

### A3 — HTTP middleware swap

`metrics::build()` at `metrics.rs:113-128` is removed. The `routes::api_routes()` invocation in `main.rs` no longer wraps with `PrometheusMetricLayer`. The metrics-side `axum::serve()` is removed entirely.

Replaced by: **`axum-otel-metrics`** (locked above). Its `HttpMetricsLayer` is layered onto `routes::api_routes()`. When no meter provider is installed (default-disabled case), the layer's measurements record into the no-op meter and never reach an exporter.

`metrics_addr` field removed from `Config`; `ATC_METRICS_ADDR` env var no longer read. Helm chart's `config.metricsAddr` and `metrics.*` block both removed; `service.metricsPort` removed; `values.schema.json` updated to drop those keys (mandatory because `additionalProperties: false`).

### A4 — Boundaries to instrument

| Site (file:line as of HEAD) | Span name | Attributes (key set; sensitive data goes nowhere) |
|---|---|---|
| `routes::webhook_handler` (`routes.rs:229`) | `webhook.handler` (root request span — see A5; **not** `#[instrument]`) | `http.route="/v1/webhooks/github"`, `webhook.delivery_id` (X-GitHub-Delivery), `webhook.event_type` |
| `atc_github::webhook::verify::verify_signature` | `webhook.verify` | `webhook.signature.present` (bool — note: effectively always `true` at this call site since `verify_signature` receives the already-extracted signature string; this attribute is a placeholder for future use or may be hoisted to `webhook.handler`), `webhook.signature.algorithm="sha256"` |
| `atc_github::webhook::parse_webhook` | `webhook.parse` | `webhook.event_type`, `webhook.action` |
| `persist::PgStore::apply_run_event` / `apply_job_event` | `persist.apply.run_event` / `persist.apply.job_event` | `run_id`, `job_id`, `seq` (set after assignment) |
| Inner `*_in_txn` helpers (existing) | nested under apply (default span via `#[instrument]`) | inherit |
| NOTIFY emit site (PgStore, after outbox INSERT) | `persist.notify.emit` | `notify.kind`, `notify.seq` |
| `listener::spawn_listener_task` (the spawned future) | `listener.task` (root, task lifetime) | none (long-lived) |
| Per-NOTIFY recv inside listener loop | `listener.recv` | `notify.payload.seq` |
| `listener::spawn_drain_task` (the spawned future) | `drain.task` (root, task lifetime) | none |
| `listener::drain_pass` | `drain.pass` (per pass) | `pass.start_floor`, `pass.rows_fetched`, `pass.batches` |
| Per-row broadcast inside `drain_pass` | `drain.broadcast` | `seq`, `kind`, `outbox_lag_ms` |

`#[tracing::instrument]` for sync entry points + `async fn`. **`tokio::spawn` boundaries** (`spawn_listener_task`, `spawn_drain_task`) wrap their futures with `.instrument(info_span!("listener.task"))` / `.instrument(info_span!("drain.task"))`.

### A5 — W3C Trace Context propagation

The handler must set the parent **before** the request span enters. Calling `tracing::Span::current().set_parent(...)` from inside an `#[instrument]` body is wrong: the span has already started, and `OpenTelemetrySpanExt::set_parent` errors after that point. Drop `#[instrument]` from `webhook_handler` and create the span manually:

```rust
use opentelemetry::propagation::TextMapPropagator;
use opentelemetry_http::HeaderExtractor;
use tracing::{info_span, Instrument};
use tracing_opentelemetry::OpenTelemetrySpanExt;

pub async fn webhook_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<serde_json::Value>) {
    // 1. Build the parent context from incoming headers BEFORE the span exists.
    let parent_cx = opentelemetry::global::get_text_map_propagator(|prop| {
        prop.extract(&HeaderExtractor(&headers))
    });

    // 2. Create the request span; attach the parent context BEFORE we enter.
    let span = info_span!(
        "webhook.handler",
        http.route = "/v1/webhooks/github",
        webhook.delivery_id = tracing::field::Empty,
        webhook.event_type = tracing::field::Empty,
    );
    span.set_parent(parent_cx);

    // 3. Run the handler body inside the span via .instrument(...).
    async move {
        // ... existing handler body, using `tracing::Span::current().record(...)` for late-bound fields ...
    }
    .instrument(span)
    .await
}
```

`HeaderExtractor` takes `&HeaderMap`; lifetime is borrowed for the duration of the `extract` call only. If `traceparent` is absent or malformed, `parent_cx` is the empty context and the resulting span is a root (gets a fresh trace ID).

This pattern is also what the listener and drain tasks use — `info_span!()` constructed at task spawn time, `.instrument(span)` wrap on the spawned future. Direct `set_parent` calls inside long-running async functions are forbidden.

### A6 — Recorder install: conditional, drop-in

When OTel is enabled (`init_otel` returned `Some`):
```rust
let meter = handles.meter_provider.meter("atc");  // Meter, not MeterProvider
let recorder = metrics_exporter_otel::OpenTelemetryRecorder::new(meter);
metrics::set_global_recorder(recorder).expect("install global metrics recorder");
```

`OpenTelemetryRecorder::new` takes a `Meter`, not the `SdkMeterProvider` itself. The meter is obtained via `meter_provider.meter("atc")` — the string is the instrumentation library scope name (per OTel conventions).

The existing `describe_*!` calls in `metrics.rs:158-249` remain — they propagate descriptions to the OTel meter via the recorder.

The custom-bucket calls (`metrics.rs:50–79`) are removed entirely. **Exponential aggregation is configured at the `SdkMeterProvider` level** via the instrument view registered in A1 step 4 — not per-metric. So `DRAIN_STARTUP_BUCKETS` and `DRAIN_SHUTDOWN_REMAINING_BUCKETS` constants are deleted, and there's no equivalent per-metric override needed (exponential histograms self-scale).

### A7 — Test fixture migration (`tests/integration/common/mod.rs`)

The 0.31 SDK exposes **`InMemorySpanExporter`** (`opentelemetry_sdk::trace::in_memory_exporter::InMemorySpanExporter`) and **`InMemoryMetricExporter`** (`opentelemetry_sdk::metrics::in_memory_exporter::InMemoryMetricExporter`) — both behind the `testing` feature on `opentelemetry_sdk`. `InMemoryMetricExporter` is a `PushMetricExporter` and must be wired inside a `PeriodicReader` (not a `ManualReader`). Tests force flush via `meter_provider.force_flush()` and use `Temporality::Delta` so that each test observes only its own delta emissions (not cumulative totals). Both expose `get_finished_*`, `reset()`, and `force_flush()`.

```rust
// Cargo.toml — backend/crates/atc-server/Cargo.toml [dev-dependencies]:
//   opentelemetry_sdk = { version = "...", features = ["testing", "spec_unstable_metrics_views"] }

use opentelemetry_sdk::metrics::in_memory_exporter::InMemoryMetricExporter;
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::trace::in_memory_exporter::InMemorySpanExporter;
use opentelemetry_sdk::trace::{SdkTracerProvider, SimpleSpanProcessor};

static OTEL_TEST_INIT: OnceLock<OtelTestHarness> = OnceLock::new();

pub struct OtelTestHarness {
    pub span_exporter: InMemorySpanExporter,
    pub metric_exporter: InMemoryMetricExporter,
    tracer_provider: SdkTracerProvider,
    meter_provider: SdkMeterProvider,
}

pub fn ensure_recorder_installed() -> &'static OtelTestHarness {
    OTEL_TEST_INIT.get_or_init(install_test_otel)
}

fn install_test_otel() -> OtelTestHarness {
    let span_exporter = InMemorySpanExporter::default();
    let tracer_provider = SdkTracerProvider::builder()
        .with_span_processor(SimpleSpanProcessor::new(span_exporter.clone()))
        .build();
    opentelemetry::global::set_tracer_provider(tracer_provider.clone());

    let metric_exporter = InMemoryMetricExporter::default()
        .with_temporality(Temporality::Delta);  // per-test delta isolation
    let metric_reader = PeriodicReader::builder(metric_exporter.clone()).build();
    let meter_provider = SdkMeterProvider::builder()
        .with_reader(metric_reader)
        // ... + view config matching production: Base2ExponentialHistogram for histograms
        .build();

    let meter = meter_provider.meter("atc");
    metrics::set_global_recorder(metrics_exporter_otel::OpenTelemetryRecorder::new(meter))
        .expect("install metrics recorder");

    opentelemetry::global::set_text_map_propagator(
        opentelemetry_sdk::propagation::TraceContextPropagator::new(),
    );

    OtelTestHarness {
        span_exporter,
        metric_exporter,
        tracer_provider,
        meter_provider,
    }
}

pub fn read_finished_spans() -> Vec<opentelemetry_sdk::export::trace::SpanData> {
    let h = ensure_recorder_installed();
    h.span_exporter.get_finished_spans().expect("get_finished_spans")
}

pub fn snapshot_metrics() -> Vec<opentelemetry_sdk::metrics::data::ResourceMetrics> {
    let h = ensure_recorder_installed();
    h.meter_provider.force_flush().expect("force_flush");
    h.metric_exporter.get_finished_metrics().expect("get_finished_metrics")
}

pub fn reset_metrics() {
    let h = ensure_recorder_installed();
    h.metric_exporter.reset();
}

pub fn reset_spans() {
    let h = ensure_recorder_installed();
    h.span_exporter.reset();
}
```

`render_metrics()` is removed entirely. `parse_unlabeled_counter` / `parse_unlabeled_gauge` are replaced by helpers that walk `ResourceMetrics → ScopeMetrics → Metric → Sum/Gauge/Histogram` and look up by name + attribute set:

```rust
pub fn counter_value(snapshot: &[ResourceMetrics], name: &str, attrs: &[KeyValue]) -> u64 { ... }
pub fn gauge_value(snapshot: &[ResourceMetrics], name: &str, attrs: &[KeyValue]) -> Option<f64> { ... }
pub fn histogram_count(snapshot: &[ResourceMetrics], name: &str, attrs: &[KeyValue]) -> u64 { ... }
pub fn histogram_sum(snapshot: &[ResourceMetrics], name: &str, attrs: &[KeyValue]) -> f64 { ... }
```

**`#[serial_test::serial]` discipline stays.** Reasons: (a) the OTel global recorders (tracer provider, meter provider, propagator) are process-wide singletons just like `metrics-exporter-prometheus` was; (b) `force_flush()` + `get_finished_*()` is non-atomic across concurrent tests — one test's flush would surface another's spans/metrics. The serial gate on tests that read snapshots remains a hard requirement.

**Test-time reset pattern.** Tests that compare deltas should call `reset_metrics()` / `reset_spans()` at start (after `ensure_recorder_installed`) so they observe only their own emissions. This is a behavior change from the Phase 5 pattern (which compared absolute baselines); the implementing subagent updates each migrated test accordingly.

### A8 — Helm chart values block

```yaml
otel:
  # -- Enable OpenTelemetry export. When false, no OTEL_* env vars are injected
  # and atc-server runs with the SDK uninstalled (no provider/exporter overhead).
  enabled: false
  # -- OTLP endpoint URL. Required when enabled. Transport is HTTP/protobuf only
  # in this issue (e.g., http://otel-collector.observability:4318).
  endpoint: ""
  # -- Service name reported via OTEL_SERVICE_NAME resource attribute.
  serviceName: "atc"
  # -- Comma-separated key=value pairs reported via OTEL_RESOURCE_ATTRIBUTES.
  # E.g., "deployment.environment=production,service.namespace=ingest".
  resourceAttributes: ""
  # -- Trace sampler. Default ParentBased(root=AlwaysOn).
  # Examples: "parentbased_traceidratio", "always_on", "always_off".
  sampler: "parentbased_always_on"
  # -- Sampler argument (e.g., "0.1" for 10% root sampling with traceidratio).
  samplerArg: ""
```

No `protocol:` key — HTTP/protobuf only in this issue (see Locked Decisions).

`templates/deployment.yaml` env vars (gated on `.Values.otel.enabled`):
```yaml
{{- if .Values.otel.enabled }}
- name: OTEL_EXPORTER_OTLP_ENDPOINT
  value: {{ .Values.otel.endpoint | quote }}
- name: OTEL_SERVICE_NAME
  value: {{ .Values.otel.serviceName | quote }}
- name: OTEL_RESOURCE_ATTRIBUTES
  value: {{ .Values.otel.resourceAttributes | quote }}
- name: OTEL_TRACES_SAMPLER
  value: {{ .Values.otel.sampler | quote }}
- name: OTEL_TRACES_SAMPLER_ARG
  value: {{ .Values.otel.samplerArg | quote }}
{{- end }}
```

**Files removed:** `templates/servicemonitor.yaml`, `tests/values-metrics.yaml`. **Service port removed:** `service.metricsPort` and the `9090`-port container declaration. **`values.schema.json` updated:** drops `metrics.*`, `config.metricsAddr`, `service.metricsPort`; adds the `otel.*` properties (endpoint as string, serviceName as string, etc.). The schema's `additionalProperties: false` setting means dropping the keys is mandatory — leaving them would not just be dead config, the schema would reject any rendered values that include them. **Chart `README.md`** updated to drop the `metrics.*` documentation block and add the `otel.*` block; `deploy/helm/atc/CLAUDE.md` updated likewise.

New chart test fixture `tests/values-otel.yaml` asserts:
- `otel.enabled: true` + endpoint set → all five `OTEL_*` env vars rendered (no `OTEL_EXPORTER_OTLP_PROTOCOL`)
- `otel.enabled: false` → none of the `OTEL_*` env vars rendered
- ServiceMonitor is not rendered
- `service.metricsPort` does not appear in any rendered manifest
- `metrics.*` values rejected by schema (the implementer adds a schema-validation test if the chart's existing test machinery supports it)

### A9 — Dedicated `just otel-dev-stack` recipe (separate from `just dev`)

`just dev` is **not modified**. It continues to start backend + frontend dev servers with no observability dependency.

A new recipe — name TBD by implementer (`otel-dev-stack`, `obs-stack`, similar) — starts a **Grafana otel-lgtm** container (`grafana/otel-lgtm`):

- One image, one container, one port surface.
- Bundles: OTel collector (OTLP receiver on `:4317` gRPC + `:4318` HTTP), Tempo (traces), Mimir/Prometheus (metrics), Loki (logs), Grafana frontend (UI on `:3000` with pre-configured datasources for all of the above).
- Choice rationale: Alloy alone has no trace-explorer UI (it has a component-status graph on `:12345` but that's not the same thing as a trace explorer). The author's *production* stack is Alloy → Tempo/Mimir/Loki — but the dev workflow benefits from a UI for traces/metrics/logs in one place without standing up the full LGTM separately. `grafana/otel-lgtm` is built exactly for this case.

Compose (or `docker run`) shape — implementer picks compose vs raw `docker run` based on what's lightest:

```yaml
# compose.otel-dev.yaml (or similar; not the only acceptable shape)
services:
  otel-dev-stack:
    image: grafana/otel-lgtm:latest
    ports:
      - "4317:4317"   # OTLP gRPC (unused by atc-server in this issue, but the image exposes it)
      - "4318:4318"   # OTLP HTTP — atc-server's target
      - "3000:3000"   # Grafana UI
```

Workflow (documented in `CONTRIBUTING.md`):
1. `just otel-dev-stack` — starts the container.
2. `OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318 just dev` (or set the env var in the shell first).
3. Open `http://localhost:3000` to inspect traces, metrics, logs.

Stop the stack with the inverse of the start command (compose down / docker stop). The recipe should also include a `stop` variant or document the shutdown command.

**No commitment to keeping the dev stack on otel-lgtm forever** — if it becomes a friction point (e.g., container size, license concerns, version drift) the recipe can swap to a different bundle without touching production code. Document the choice as "default dev stack, swap-friendly."

### A10 — Cooperative shutdown integration

`shutdown::run_shutdown_orchestration` (per PR #81) gains an OTel shutdown step. After the existing task drains complete:
```rust
if let Some(handles) = otel_handles {
    handles.tracer_provider.shutdown();
    handles.meter_provider.shutdown();
}
```

**Ordering principle:** "no live emitter when shutdown fires." Concretely, the shutdown orchestration must verify the following emitters have all joined before calling provider shutdown:
1. Drain task (`spawn_drain_task` JoinHandle) — final `drain.broadcast` spans and `atc_pg_drain_shutdown_remaining_rows` observation.
2. Listener task (`spawn_listener_task` JoinHandle).
3. Process collector (`metrics::spawn_process_collector` JoinHandle).
4. axum graceful-shutdown drain — once axum returns from `into_future().await`, no new request handler will start, so no new HTTP middleware emissions begin. In-flight handlers complete before that returns.

Then call `tracer_provider.shutdown()` → `meter_provider.shutdown()` → process exit. Add a comment block in `shutdown.rs` enumerating these emitters so a future contributor adding a new emitter knows to extend the join set.

`OtelHandles` is consumed by `run_shutdown_orchestration` after the four emitters join. A `pub fn otel::shutdown(handles)` helper exists so tests can drive the shutdown path without standing up the full orchestration.

## Implementation Phases

Each phase is a self-contained subagent dispatch. Each phase begins with TDD red, ends with green + lints/tests passing. Subagents read this plan + the cited file:line references and dispatch their own research subagents only when needed.

### Phase 1 — OTel SDK initialization (red+green)

**Tests (red):**
- `tests/integration/otel_init_test.rs`:
  - `metrics_macro_is_noop_with_no_endpoint`: with `OTEL_EXPORTER_OTLP_ENDPOINT` unset, calling `metrics::counter!("test").increment(1)` does not panic and is a no-op.
  - `init_otel_returns_none_with_no_endpoint`: `init_otel(&cfg)` returns `None`.
  - `init_otel_returns_some_with_endpoint`: `init_otel(&cfg_with_endpoint)` returns `Some(handles)`.

**Implementation (green):**
- `cargo add opentelemetry opentelemetry_sdk opentelemetry-otlp tracing-opentelemetry metrics-exporter-otel opentelemetry-http opentelemetry-semantic-conventions` (no version pinning, rule 3).
- `backend/crates/atc-server/src/otel.rs` new module per A1.
- `lib.rs` exports it.
- `main.rs` wires it after subscriber init, composing the OpenTelemetryLayer when present.

### Phase 2 — Boundary instrumentation + W3C propagation (red+green)

**Tests (red):**
- `tests/integration/tracing_webhook_spans_test.rs`:
  - `webhook_post_emits_expected_span_hierarchy`: POST a webhook, assert in-memory span exporter has `webhook.handler` root with `webhook.verify`, `webhook.parse`, `persist.apply.run_event` children. Resource attribute `service.name=atc`.
  - `traceparent_header_propagates_to_root_span`: POST with `traceparent: 00-<hex32>-<hex16>-01`, assert the root span's trace ID matches the header.
  - `drain_pass_span_is_child_of_drain_task`: insert outbox row, trigger drain, assert `drain.broadcast` ← `drain.pass` ← `drain.task` chain (covers the `tokio::spawn` Instrument-trait gotcha).

**Implementation (green):**
- `#[tracing::instrument]` annotations per A4 table.
- W3C extraction in webhook_handler per A5.
- `.instrument(span)` wrapping on the futures spawned in `spawn_listener_task` and `spawn_drain_task`.

### Phase 3 — Recorder swap + HTTP middleware swap + `/metrics` removal + test-fixture migration (single phase, single green checkpoint)

**Why merged:** the test fixture rewrite (`tests/integration/common/mod.rs`) introduces helpers (`snapshot_metrics`, `read_finished_spans`, etc.) that depend on the OTel recorder being installed. The recorder install lives behind the production swap (remove `metrics-exporter-prometheus`, add `metrics-exporter-otel`). Splitting fixture migration from recorder swap leaves a Phase 3 that cannot compile (helpers reference types that aren't in `Cargo.toml` yet) — and the project's planning workflow requires every phase to end green. So fixture migration + recorder swap + middleware swap + endpoint removal land in one phase, one subagent dispatch, one green commit.

**Tests (red):**
- All ~20 test files that import from `tests/integration/common/mod.rs` for `render_metrics`/`PROMETHEUS_INIT`/`metrics_addr` (enumerated in DoD #9) updated to the new snapshot helpers. Tests that hit the legacy `/metrics` HTTP-side-port (`tests/integration/metrics.rs`, the `metrics_router_isolation.rs` HTTP-side check) are deleted, not migrated — the endpoint is gone.
- `tests/integration/no_metrics_endpoint_test.rs` (new): assert no `/metrics` route exists on any router; assert the metrics-side listener is not started; assert `Config` has no `metrics_addr` field.

**Implementation (green):**
- `cargo remove axum-prometheus metrics-exporter-prometheus`.
- `cargo add axum-otel-metrics metrics-exporter-otel` (and any updates to `opentelemetry_sdk` features needed for `testing` and `spec_unstable_metrics_views`).
- `tests/integration/common/mod.rs` per A7: replace `PROMETHEUS_INIT` with `OTEL_TEST_INIT`; replace `render_metrics` + `parse_unlabeled_*` with `snapshot_metrics`/`read_finished_spans` and the typed lookup helpers.
- `metrics.rs::build()` deleted along with `install_recorder()`, `PROMETHEUS_CONTENT_TYPE`, `DRAIN_STARTUP_BUCKETS`, `DRAIN_SHUTDOWN_REMAINING_BUCKETS`.
- Metrics-side `axum::serve()` invocation in `main.rs` removed.
- `Config.metrics_addr` field removed; figment env-var coverage updated; `config_tests.rs` updated.
- `routes::api_routes()` layered with `axum-otel-metrics::HttpMetricsLayer`.
- `init_otel()` registers the `metrics-exporter-otel` recorder per A6 (Meter, not provider).
- `register_pg_write_counters()` and `register_listener_metrics()` (the `describe_*!` registrations) called after recorder install in the OTel-enabled path; in the disabled path they're no-ops.

**Acceptance:** `cargo nextest run -p atc-server` green at the end of this phase. The phase is large; the implementing subagent splits the work internally if needed but lands one PR-portable commit.

### Phase 4 — Cooperative shutdown integration

**Tests:**
- `tests/integration/shutdown_otel_flush_test.rs`: emit a span + a metric; trigger shutdown; assert the in-memory exporter recorded both before "process exit" (the test's drop-time check).

**Implementation:**
- `shutdown::run_shutdown_orchestration` gains the OTel shutdown step per A10.

### Phase 5 — Helm chart + dev compose

**Tests:**
- `deploy/helm/atc/tests/values-otel.yaml` + the chart's existing `helm template` test infrastructure asserts the rendered surface per A8.
- New chart-test assertions: ServiceMonitor not rendered, `service.metricsPort` absent.

**Implementation:**
- `values.yaml` `otel.*` block per A8.
- `templates/deployment.yaml` env var injection per A8 (5 env vars; no `OTEL_EXPORTER_OTLP_PROTOCOL`).
- `templates/servicemonitor.yaml` deleted.
- `templates/service.yaml` metricsPort removed.
- `tests/values-metrics.yaml` deleted; new `tests/values-otel.yaml` added.
- `values.schema.json` updated: drop `metrics.*`, `config.metricsAddr`, `service.metricsPort`; add `otel.*` properties.
- Chart `README.md` updated: drop `metrics.*` documentation, add `otel.*` block; signal breaking change.
- New `just otel-dev-stack` recipe per A9 (does NOT modify `just dev`).
- Compose file (or `docker run` invocation) for `grafana/otel-lgtm` per A9.
- `CONTRIBUTING.md` documents the dev-stack workflow (run the stack in a side terminal, set `OTEL_EXPORTER_OTLP_ENDPOINT`, then run `just dev`).

### Phase 6 — Documentation sweep

Update files per "Documents to Update" below. This phase is doc-only and runs last to avoid churn from earlier phases.

## Acceptance Criteria

### Tracing
- **AC1.** `helm install` with `otel.enabled: true, otel.endpoint: http://collector:4318` and a webhook POST produces an OTLP HTTP request to the collector with the span hierarchy from A4. `service.name=atc` resource attribute set.
- **AC2.** With `traceparent: 00-<hex32>-<hex16>-01` in the request, the trace ID of the root span matches the header's trace-id.
- **AC3.** Drain pipeline spans (`drain.pass`, `drain.broadcast`) are children of `drain.task`, not fresh roots — covering the `tokio::spawn` Instrument-trait gotcha.
- **AC4.** With `OTEL_EXPORTER_OTLP_ENDPOINT` unset: no OTel SDK initialization. `tracing_subscriber::fmt()` continues to log structured logs to stderr unchanged. `init_otel()` returns `None`.

### Metrics
- **AC5.** All `atc_pg_*` metrics + `atc_build_info` emit via the OTLP exporter with names, types, and attributes preserved from the Phase 5 inventory. The migrated integration tests verify each.
- **AC6.** Histogram metrics emit as OTel `Base2ExponentialHistogram` aggregations (verifiable via the in-memory metric exporter inspecting `Histogram` data type and `data_points[].positive` / `negative` buckets being exponential, not explicit). The OTLP→Prometheus remote-write path produces native histograms in Prometheus 2.40+ — covered by AC24 (manual smoke against Alloy).
- **AC7.** `git grep "set_buckets_for_metric"` returns zero hits in `backend/`. `git grep "PrometheusBuilder\|PrometheusMetricLayer\|PrometheusHandle"` returns zero hits in `backend/`. (Plan file lives in `docs/design-plans/` so its mentions are not in `backend/`.)
- **AC8.** With `OTEL_EXPORTER_OTLP_ENDPOINT` unset, `metrics::counter!("any_metric").increment(1)` does not panic and produces no observable side effect (no SDK / no provider / no exporter / no background-task overhead — but the macro itself still resolves through the global recorder pointer at minimal cost; AC verifies behavior, not "zero cost" in the literal sense).

### Endpoint and HTTP middleware
- **AC9.** `git grep "PROMETHEUS_CONTENT_TYPE\|metrics_addr\|MetricsAddr\|ATC_METRICS_ADDR"` returns zero hits in `backend/`. The `/metrics` route is not in any router. `Config` has no `metrics_addr` field.
- **AC10.** HTTP request duration emits with OTel HTTP semantic-conventions attributes — `http.request.method`, `http.response.status_code`, `http.route` — verifiable via the in-memory metric exporter. (`url.scheme` is recorded on `http.server.active_requests`, not on request duration, per `axum-otel-metrics 0.13` behavior.)

### Helm chart
- **AC11.** `helm template . -f tests/values-otel.yaml` renders deployment with all five `OTEL_*` env vars set from values: `OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_SERVICE_NAME`, `OTEL_RESOURCE_ATTRIBUTES`, `OTEL_TRACES_SAMPLER`, `OTEL_TRACES_SAMPLER_ARG`. (No `OTEL_EXPORTER_OTLP_PROTOCOL` — HTTP/protobuf only.)
- **AC12.** ServiceMonitor template is removed. `git grep "ServiceMonitor\|servicemonitor" deploy/helm/` returns hits only in chart README / CHANGELOG / migration notes.
- **AC13.** Service manifest no longer carries a `metricsPort`. `git grep "metricsPort\|metrics_addr\|9090" deploy/helm/atc/templates/` returns zero hits.
- **AC14.** `values.schema.json` updated: drops `metrics`, `config.metricsAddr`, `service.metricsPort`; adds `otel` (with `enabled`, `endpoint`, `serviceName`, `resourceAttributes`, `sampler`, `samplerArg` properties). `tests/values-metrics.yaml` deleted; `tests/values-otel.yaml` added.

### Test discipline
- **AC15.** Every test file in `backend/crates/atc-server/tests/integration/` that imports `render_metrics`, `PROMETHEUS_INIT`, `metrics_addr`, or `ATC_METRICS_ADDR` from `common/mod.rs` is migrated to the new OTel snapshot helpers. The implementer's tracking grep (`grep -l 'render_metrics\|PROMETHEUS_INIT\|metrics_addr\|ATC_METRICS_ADDR' backend/crates/atc-server/tests/integration/`) returns zero hits at the end of the phase.
- **AC16.** New `tracing_webhook_spans_test.rs`, `otel_init_test.rs`, `no_metrics_endpoint_test.rs`, `shutdown_otel_flush_test.rs` exist and pass.
- **AC17.** `cargo nextest run -p atc-server` passes (number adjusted from 327+ to whatever the real count is post-removals; the legacy `/metrics` HTTP-side-port test set is deleted, so test count drops).

### Cooperative shutdown
- **AC18.** `shutdown::run_shutdown_orchestration` calls `tracer_provider.shutdown()` and `meter_provider.shutdown()` after task drains complete, before process exit. The shutdown_otel_flush_test verifies in-flight emission survives shutdown.

### Documentation
- **AC19.** `docs/architecture/metrics.md` (the **canonical** doc-contract home — already mapped from `metrics.rs` at `scripts/doc-mapping.sh:28`) is rewritten for OTLP semantics and **extended in place** to cover the span authoring contract. The contract subsection becomes "Metric and span authoring contract." `docs/architecture/backend-server.md` gets a new § Tracing that links back to `metrics.md` for the contract — it does **not** duplicate the contract.
- **AC20.** `scripts/doc-mapping.sh` mapping for `backend/crates/atc-server/src/otel.rs` is decided: either it lives under the existing generic `atc-server/src/* -> backend-server.md` rule (in which case no edit needed but verified explicitly), or — preferred — it gets its own mapping to **both** `backend-server.md` AND `metrics.md` (the canonical contract home), reflecting that changes to OTel init affect both surfaces.
- **AC21.** `backend/crates/atc-server/CLAUDE.md` updated: metrics inventory marked as OTel-emitted; new spans inventory section; `OTEL_*` env-var contract documented.
- **AC22.** `deploy/helm/atc/CLAUDE.md` updated with `otel.*` values reference; deletion of `metrics.*` values noted.
- **AC23.** `CONTRIBUTING.md` § Metrics renamed → § Observability covering both metrics and spans, cross-linking to `docs/architecture/metrics.md`.
- **AC24.** Chart `README.md` updated: drop the `metrics.*` documentation block; add the `otel.*` block; signal the breaking-change in the chart's CHANGELOG (release-please will pick up the conventional-commit `feat!:` or `BREAKING CHANGE:` footer).

### Operational
- **AC25.** Manual smoke test against the author's homelab Alloy: ATC pod with `OTEL_EXPORTER_OTLP_ENDPOINT` set produces traces visible in Alloy / Tempo. All Phase 5 metrics visible at the collector's Prometheus exposition with native histograms (verifiable by querying for `*_bucket` series being absent on the histograms — native histograms do not emit bucket lines in Prometheus exposition).

## Documents to Update

| Document | Change |
|---|---|
| `docs/architecture/metrics.md` (**canonical contract home**) | Rewrite for OTel/OTLP semantics; preserve metric inventory; remove text-format/Prometheus-recorder references; **extend the "Metric authoring contract" → "Metric and span authoring contract"** in place (do not relocate) |
| `docs/architecture/backend-server.md` | New § Tracing covering instrumentation boundaries, propagator config, sampler choice; existing § Metrics simplified to a pointer at `metrics.md`; remove `axum-prometheus` references |
| `docs/architecture/deployment.md` | OTel collector dependency note; remove ServiceMonitor references; document the `OTEL_*` env-var surface |
| `docs/architecture/ci-pipeline.md` | If it references `/metrics` as part of the operator surface, update to reference the collector's exposition |
| `CONTRIBUTING.md` | § Metrics → § Observability (metrics + spans); cross-link to `docs/architecture/metrics.md` |
| `scripts/doc-mapping.sh` | Decide mapping for `backend/crates/atc-server/src/otel.rs` (per AC20); verify existing `metrics.rs → metrics.md` mapping survives the file's shrinkage |
| `backend/crates/atc-server/CLAUDE.md` | Metrics inventory now OTel-emitted; new spans inventory; `OTEL_*` env-var contract; OTel SDK shutdown contract |
| `deploy/helm/atc/CLAUDE.md` | New `otel.*` values block reference; `metrics.*` deletion noted |
| `deploy/helm/atc/README.md` | Drop `metrics.*` block; add `otel.*` block; signal breaking change |
| `deploy/helm/atc/values.schema.json` | Drop `metrics`, `config.metricsAddr`, `service.metricsPort`; add `otel` properties |
| `README.md` | Updated quickstart if it references `/metrics` |

## Implementation Guidance

Apply rules from `docs/implementation-guidance.md`:
- **Rule 1:** feature branch; squash-merge PR; PR title is the feature title.
- **Rule 2:** TDD — every phase starts with red tests, then green.
- **Rule 3:** **NEVER pin library versions.** Use `cargo add opentelemetry`, `cargo add tracing-opentelemetry`, etc. **But the OTel ecosystem deliberately offsets versions** (`tracing-opentelemetry 0.32.1` supports both 0.31 and 0.32; the one-version lag of earlier releases no longer applies), and the HTTP middleware crates split across trains (`axum-otel-metrics` on 0.31, `tower-otel-http-metrics` on 0.30). Resolved versions must be verified before the first compile. Before the first `cargo build`: run `cargo add --dry-run` for each new crate, verify all OTel crates resolve to compatible major versions (0.31 train as locked above), and fix conflicts by re-running `cargo add` with explicit minor selectors (e.g., `cargo add opentelemetry@0.31`). Pinning at the minor level is acceptable and not a Rule-3 violation; pinning at the patch level is.
- **Rule 4:** doc-mapping update for `otel.rs`.
- **Rule 7:** if `tracing_webhook_spans_test.rs` exceeds ~500 lines, split by concern.
- **Rule 14:** dispatch subagents per phase to keep main context clean.
- **Rule 16:** prefer `ed3d-research-agents:*` for any investigation.
- **Rule 17:** strip planning-artifact labels (Phase N, AC N) from current-state artifacts (tests, comments, docs).

Apply project-memory feedback:
- `feedback_dont_assume_dep_minimalism.md` — `metrics-exporter-otel`, `axum-otel-metrics` are batteries-included; don't hand-roll.
- `feedback_use_just_test_or_nextest.md` — `cargo nextest run`, not bare `cargo test`.
- `feedback_no_source_grep_tests.md` — assert metric/span emission via in-memory exporter, not by grepping source for macro calls.
- `feedback_codex_review_before_exit.md` — codex `xhigh` review of THIS plan before ExitPlanMode.
- `feedback_verify_lefthook_installed.md` — run `just setup` at the start of the new worktree.
- `feedback_run_e2e_tests_for_frontend_changes.md` — N/A (no frontend changes in this issue).
- `feedback_no_pip_install_in_agents.md` — N/A (Rust-only).

## Out of Scope

- **Frontend OTel browser SDK** — separate integration; deferred per issue body.
- **Bundling an OTel collector in the chart** — operators bring their own; chart documents the dependency.
- **Tail-based / span-linked sampling** — start with head-based ratio; revisit if data shows inadequacy.
- **Cross-replica span linking** via `min_pending_seq` / drain-pass spans — interesting but separate design.
- **Logs export to OTLP** via `opentelemetry-appender-tracing` — operators (incl. author's homelab Alloy) collect logs from stdout via Loki; defer.
- **Removing the `metrics` crate facade** — keeping it. The macro syntax decouples ATC from any single emission backend.
- **Direct OTel SDK metric emission** (bypassing `metrics-rs`) — the `metrics-exporter-otel` drop-in is the locked decision.
- **Outbox retention, in-memory mode removal, raw-webhook persistence** — separate Phase 5 sub-plans, unrelated to OTel.
- **OTLP gRPC transport** — HTTP/protobuf only in this issue. gRPC support requires `opentelemetry-otlp` feature-flag work (`grpc-tonic`) and a `protocol:` chart values key; deferred to a follow-up if a downstream operator needs it.
- **Re-enabling stable OTel histogram views** — the plan locks `spec_unstable_metrics_views`; if that feature is stabilized in a later `opentelemetry_sdk` release, switching off the unstable flag is a follow-up.

## Glossary

- **OTLP** — OpenTelemetry Protocol; the wire format for traces/metrics/logs export. HTTP/protobuf or gRPC.
- **W3C Trace Context** — IETF standard for trace context propagation across HTTP services. The `traceparent` header carries trace-id + span-id + flags.
- **Sampler** — decides whether a trace is sampled (recorded and exported) or dropped at the source. `ParentBased` respects the parent's decision; root samplers fire when no parent.
- **Exponential histogram** — OTel's native histogram type with adaptive bucket boundaries (logarithmic spacing). Maps to Prometheus native histograms in 2.40+.
- **Resource attributes** — span/metric labels that describe the emitting process (`service.name`, `deployment.environment`, etc.). Set once per process, not per span.
- **Alloy** — Grafana's OpenTelemetry collector distribution. The author's homelab runs it; the chart's deployment story assumes operators have a similar collector.
- **Two-recorder hazard** — running both `metrics-exporter-prometheus` AND OTel emission in parallel duplicates pipelines and can drift. This plan eliminates the hazard by removing the Prometheus recorder.
