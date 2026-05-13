## Context

Every `metrics::counter!()` / `gauge!()` / `histogram!()` macro invocation in `atc-server/src/**` (31 emit sites today: 15 in `persist/pg.rs`, 15 in `listener.rs`, 1 in `register_build_info`) walks the recorder, performs a `Key`-based shard lookup in the metrics-rs `Registry`, and clones the handle's internal `Arc<dyn …Fn>`. For hot paths — the drain loop's per-row broadcast, the listener's per-NOTIFY handler, the webhook commit path — this happens once per emit.

The metrics-rs docs recommend caching the `Counter` / `Gauge` / `Histogram` handle (each is internally `Arc<dyn …Fn>`; `.clone()` is cheap) and calling `.increment()` / `.set()` / `.record()` on the cached handle. The macro form is documented as ergonomic-but-slower; caching skips the registry lookup after the first call.

The primary motivation is **defense in depth, not measured performance**. PR #153 fixed a real correctness bug where `metrics-util 0.20.1` had a `Registry` hash-contract mismatch (`Key::hash` stored entries at hash H1 but `KeyHasher` probed at H2, so every emit past the first installed a fresh observable callback that `metrics-exporter-otel` overwrote with its own value). The bug surfaced *only* in the inline-macro form because the cached form hits the registry exactly once at handle creation. Converting every emit site to cached handles eliminates the entire class.

Issue #155 captures this and lists the ACs. The author has explicitly **dropped the bench AC** as over-reaching (this is correctness-shaped defense-in-depth, not micro-perf; existing CI is the non-regression gate).

## Definition of Done

1. Every `metrics::counter!("name")…` / `metrics::gauge!("name")…` / `metrics::histogram!("name")…` emit site in `backend/crates/atc-server/src/**` goes through a cached handle held on the `PgMetrics` struct.
2. Two documented inline exceptions remain:
   - `describe_*!` macros (metadata-only; called once at startup).
   - `register_build_info`'s `metrics::gauge!(…).set(1.0)`. This *is* a real emit (not metadata-only), but it is one-shot at startup with compile-time labels and is never touched again — so caching its handle would be pure ceremony. The architecture doc must state this rationale verbatim so future contributors don't misread "one-shot at startup" as the same exception class as `describe_*!`.
3. `docs/architecture/metrics.md` records the cached-handle convention so a future contributor doesn't reintroduce inline emits.
4. The existing PG-metric integration tests continue to pass with no behavioral regression (see [Regression net](#regression-net) below for the specific suites).
5. `cargo clippy -p atc-server -- -D warnings` clean.

Explicitly **out of scope** (matches issue):
- The bench AC is dropped per author direction.
- Test code that emits metrics inline (`tests/integration/otel_init_test.rs`) stays as-is.
- Metric names, attributes, and label sets do not change — this is a pure refactor.
- `atc_pg_in_memory_drift_total` is described but never emitted today; it stays described, no handle cached.

## Locked Decisions

- **Single `PgMetrics` struct, owned by `PgStore`, shared with task closures via `Arc<PgMetrics>`.** Rationale: PgStore is the actual runtime owner of both the write path AND the spawned listener+drain tasks. A split into `PgWriteMetrics` + `PgListenerMetrics` would mirror the cosmetic `register_*` grouping but invent an ownership boundary the code does not have. `atc_pg_min_pending_seq` and `atc_pg_broadcast_watermark` already straddle both paths (writer-side init, drain-side mirror) and would be arbitrarily assigned in a two-struct shape.
- **`register_build_info` stays a free function in `metrics.rs`.** The one-shot `gauge.set(1.0)` is a real emit; it is safe to leave inline because it executes exactly once at startup and is never invoked again. (Distinct rationale from `describe_*!`, which is genuinely metadata-only.)
- **`PgStore` owns `PgMetrics` construction.** `PgStore::start` (and `start_with_test_hooks`) internally calls `PgMetrics::register()`; the result is stored as a field and cloned into the listener/drain closures. Not threaded as a parameter from `main.rs`. Rationale: every `atc_pg_*` emit site is PgStore-owned (writes in `apply_*_event`, listener task, drain task — all spawned and joined by PgStore per ADR-0006). Locality wins over plumbing. The recorder-install precondition is already a PgStore precondition: production `main.rs` runs `init_otel` before `PgStore::start`, and the integration harness installs the recorder once per binary via the `OnceLock` guard at `tests/integration/common/mod.rs:100` before any test constructs a PgStore.
- **No new struct on `InMemoryStore`.** In-memory mode emits zero metrics today (verified by inventory); leaving it untouched avoids scope creep.

## Architecture

### Emit-site inventory (validated)

`backend/crates/atc-server/src/persist/pg.rs` — 14 sites in `PgStore::apply_run_event` / `apply_job_event` plus 1 in `start_inner`:

| Metric | Lines | Labels |
|---|---|---|
| `atc_pg_write_failures_total` | 485, 491, 495, 503, 510, 514, 537, 543, 547, 555, 562, 566 | `kind` ∈ {parity, transient} |
| `atc_pg_notify_emitted_total` | 518, 570 | `kind` ∈ {run, job} |
| `atc_pg_broadcast_watermark` | 302 | none (initial set from `start_inner`) |

`backend/crates/atc-server/src/listener.rs` — 15 emit sites split across listener-task and drain-task closures:

| Metric | Lines | Type |
|---|---|---|
| `atc_pg_listener_recv_errors_total` | 107 | counter |
| `atc_pg_notify_received_total` | 133 | counter |
| `atc_pg_wake_coalesced_total` | 142 | counter |
| `atc_pg_min_pending_seq` | 155, 253, 325 | gauge (listener + drain + drain-error path) |
| `atc_pg_drain_pass_duration_seconds` | 275 | histogram |
| `atc_pg_drain_startup_seconds` | 279 | histogram |
| `atc_pg_drain_passes_total` | 284 | counter |
| `atc_pg_broadcast_watermark` | 305 | gauge |
| `atc_pg_drain_shutdown_remaining_rows` | 380 | histogram |
| `atc_pg_drain_unknown_kind_total` | 495 | counter |
| `atc_pg_drain_duplicate_skipped_total` | 502 | counter |
| `atc_pg_outbox_lag_seconds` | 533 | histogram |
| `atc_pg_drain_rows_total` | 545 | counter |

`backend/crates/atc-server/src/metrics.rs` — 1 site in `register_build_info` at line 16 (exception per Locked Decisions).

### Regression net

The behavioral safety net for this refactor lives in the PG-metric integration tests, NOT in `shutdown_otel_flush_test.rs` (that test installs a local `SdkMeterProvider` and never exercises `PgStore` or any `atc_pg_*` metric — it is irrelevant here):

| Suite | Asserts |
|---|---|
| `tests/integration/transactional_writes_tests.rs` | `atc_pg_write_failures_total{kind}`, `atc_pg_notify_emitted_total{kind}` post-write values |
| `tests/integration/outbox_tests.rs` | Drain-path counter increments across batches |
| `tests/integration/metrics_broadcast_watermark_test.rs` | `atc_pg_broadcast_watermark` gauge advances after broadcast |
| `tests/integration/metrics_min_pending_seq_test.rs` | `atc_pg_min_pending_seq` set/swap discipline |
| `tests/integration/metrics_drain_pass_duration_test.rs`, `metrics_drain_startup_test.rs`, `metrics_drain_shutdown_remaining_test.rs`, `metrics_outbox_lag_test.rs`, `metrics_wake_coalesced_test.rs` | Per-metric histogram/counter assertions |
| `tests/integration/gap_healing.rs` | End-to-end drain + listener counter behavior under gap-healing |

These are the tests that must pass before and after the refactor. A botched conversion (e.g. a handle wired before recorder install → permanent no-op, or a missed emit site) shows up here.

### Target shape

```rust
// backend/crates/atc-server/src/metrics.rs

use std::sync::Arc;
use metrics::{Counter, Gauge, Histogram};

/// Cached metric handles for every repeat-emit site in PG mode.
///
/// Constructed once at startup (after the recorder is installed by
/// `otel::init_otel`) via `PgMetrics::register()`. Cloned cheaply (each field
/// is internally `Arc<dyn …Fn>`) into `PgStore` and into the listener/drain
/// task closures.
pub struct PgMetrics {
    // Write-path counters (PgStore::apply_*_event), 4 unique (name, label) tuples.
    pub write_failures_parity: Counter,
    pub write_failures_transient: Counter,
    pub notify_emitted_run: Counter,
    pub notify_emitted_job: Counter,

    // Listener-task counters.
    pub notify_received: Counter,
    pub listener_recv_errors: Counter,
    pub wake_coalesced: Counter,

    // Drain-task counters.
    pub drain_passes: Counter,
    pub drain_rows: Counter,
    pub drain_duplicate_skipped: Counter,
    pub drain_unknown_kind: Counter,

    // Histograms (drain task, plus one shutdown observation).
    pub outbox_lag: Histogram,
    pub drain_pass_duration: Histogram,
    pub drain_startup: Histogram,
    pub drain_shutdown_remaining_rows: Histogram,

    // Gauges (writer-side init + drain-side mirror).
    pub broadcast_watermark: Gauge,
    pub min_pending_seq: Gauge,
}

impl PgMetrics {
    /// Describes every metric and caches its handle.
    ///
    /// MUST be called after the recorder is installed — handles cached before
    /// recorder install bind permanently to the no-op recorder. PgStore's
    /// constructor satisfies this precondition: production `main.rs` runs
    /// `init_otel` before `PgStore::start`, and the integration test harness
    /// installs the recorder once per binary before any test constructs a
    /// `PgStore`.
    ///
    /// Safe to call multiple times (e.g., multiple `PgStore` instances across
    /// tests): `metrics-exporter-otel`'s metadata table overwrites the
    /// `(KeyName, MetricKind)` entry on repeated describes, and the
    /// `metrics-util` registry deduplicates handle creation by `Key`. So every
    /// call returns equivalent handles bound to the same underlying registry
    /// entries.
    pub(crate) fn register() -> Arc<Self> {
        // describe_*!() per metric name, then field = metrics::<type>!(name, [labels]).
    }
}
```

Call sites collapse from `metrics::counter!("atc_pg_write_failures_total", "kind" => "parity").increment(1)` to `self.metrics.write_failures_parity.increment(1)`.

### Plumbing

- `main.rs` (after `init_otel`):
  ```rust
  register_build_info();                       // unchanged — describes + sets the one-shot gauge
  let pg_store = PgStore::start(pool, cfg).await?;
  // PgStore::start_inner internally calls PgMetrics::register() and stores Arc<PgMetrics>.
  ```
- `PgStore` gains a field `metrics: Arc<PgMetrics>`. `PgStore::start` / `start_with_test_hooks` signatures are **unchanged** — construction is internal.
- `PgStore::start_inner` calls `PgMetrics::register()` once, stores the resulting `Arc<PgMetrics>` on the struct, and clones it once for the listener-task closure and once for the drain-task closure.
- `spawn_listener_task` and `spawn_drain_task` (the free functions in `listener.rs`) gain `metrics: Arc<PgMetrics>` parameters.
- **Recorder-install precondition:** Already a PgStore precondition. No new contract to advertise — `init_otel` (prod) and the harness's `OnceLock` guard (tests) both run before any PgStore construction, which is where `PgMetrics::register()` now fires.

### Test harness (semantic change)

`tests/integration/common/mod.rs:118-120` currently calls three describe-only free functions (`register_build_info`, `register_pg_write_counters`, `register_listener_metrics`) — pure metadata, no live instrument creation.

After the refactor, the harness keeps only:
```rust
atc_server::metrics::register_build_info();
```
The two PG-specific free functions are deleted. `PgMetrics::register()` is invoked transitively whenever a test constructs a `PgStore` via `start_with_test_hooks`.

**Semantic change:** The pre-refactor harness installed describes (metadata) at binary-init time; instruments came into existence lazily on the first inline `metrics::counter!()` emit during a test. After the refactor, all PG instruments come into existence at `PgStore::start` time — earlier than the first emit but later than harness setup. Under `metrics-exporter-otel`, instrument creation installs an observable callback that exports the instrument's *current value* at every flush — `0` for counters/gauges that have never been incremented, no observation for histograms.

**Implication for the regression net:** any test that previously relied on "metric absent until first emit" or "startup-seeded zero never observed" will now see a `0` observation from PgStore construction, before the test body has done meaningful work. The plan does NOT pre-judge which tests this affects; the implementation MUST audit the suites listed under [Regression net](#regression-net) and convert any presence-based assertion to a *delta-based* assertion (read the value before the action, read it after, assert on the difference). Specifically suspect: `transactional_writes_tests.rs` post-write value checks and `metrics_broadcast_watermark_test.rs` startup-seeding checks. The audit happens in Phase 1 (baseline run) so any pre-existing flakes don't conflate with the refactor.

### Rejected alternatives

- **`OnceLock<Counter>` per metric (issue's option 1):** Equivalent perf (single relaxed atomic load on the common path), but handles are scattered across `pg.rs` / `listener.rs` / `metrics.rs`. Adding a new metric requires remembering to declare a `OnceLock` next to a call site instead of editing one struct.
- **`PgWriteMetrics` + `PgListenerMetrics` split (issue's option 2):** Mirrors the existing `register_pg_write_counters` / `register_listener_metrics` grouping cosmetically, but invents an ownership boundary PgStore doesn't have. The shared metrics (`min_pending_seq`, `broadcast_watermark`) would need to live in one struct and be access-leaked into the other.

## Implementation Phases

1. **Baseline + harness-semantic audit.** Run `just test` against the current macro-based implementation. For each suite in [Regression net](#regression-net), classify each metric-touching assertion as "delta-based" (value-before vs. value-after) or "presence-based" / "startup-zero-seeded" (the latter two are at risk of breaking when `PgMetrics::register()` creates live instruments at harness setup). Record the at-risk assertions. Add minimal value-before reads if a suite has no delta path today — these are the failing-tests-equivalent additions.
2. **Introduce `PgMetrics` struct (additive only).** Add struct definition + `PgMetrics::register()` impl in `metrics.rs`. Delete `register_pg_write_counters` and `register_listener_metrics` in the same commit (their describes move into `PgMetrics::register`). `register_build_info` stays. Crate still compiles; no call site uses `PgMetrics` yet.
3. **Wire `PgMetrics` into `PgStore` AND convert at-risk assertions in the same change.** Delete `register_pg_write_counters` + `register_listener_metrics` calls from `tests/integration/common/mod.rs`; keep only `register_build_info()`. Add `metrics: Arc<PgMetrics>` field on `PgStore`; have `start_inner` call `PgMetrics::register()` and store the result. Add `metrics: Arc<PgMetrics>` parameter to `spawn_listener_task` and `spawn_drain_task` (`start_inner` clones it into each closure). `PgStore::start` / `start_with_test_hooks` signatures unchanged. Convert every at-risk assertion identified in Phase 1 to a delta-based read. Run `just test`; everything passes — existing emit sites still use the macro form, so values are identical, but instruments now exist from PgStore construction onward.
4. **Convert `persist/pg.rs` emit sites.** Replace the 14 `metrics::counter!()` calls in `apply_run_event` / `apply_job_event` and the 1 in `start_inner` with cached-handle field accesses (`self.metrics.<field>`). Run `cargo nextest run -p atc-server` after this commit.
5. **Convert `listener.rs` emit sites.** Replace the 15 `metrics::counter!()` / `gauge!()` / `histogram!()` calls in `spawn_listener_task` and `spawn_drain_task` closures with `metrics.<field>` field accesses on the `Arc<PgMetrics>` parameter. Run nextest.
6. **Update documentation.** Add a "Cached handle convention" subsection under "Metric and span authoring contract" in `docs/architecture/metrics.md`, including the explicit `register_build_info` rationale (one-shot startup emit, not "metadata-only"). Update the `atc-server/CLAUDE.md` `metrics` module description to mention `PgMetrics`. Verify `scripts/check-docs-lefthook.sh` accepts the diff.
7. **Final verification.** `just lint` (clippy + biome + rustfmt), `just test` (backend nextest + frontend Vitest), and the grep gate: `git grep -E 'metrics::(counter|gauge|histogram)!' backend/crates/atc-server/src/` returns only the `register_build_info` site at `metrics.rs:16`.

## Acceptance Criteria

- **AC1.** `git grep -E 'metrics::(counter|gauge|histogram)!' backend/crates/atc-server/src/` returns **only** the `atc_build_info` site inside `register_build_info` (one match, in `metrics.rs`). Every other production emit goes through `PgMetrics`.
- **AC2.** `git grep -E 'metrics::describe_(counter|gauge|histogram)!' backend/crates/atc-server/src/` returns hits only inside `register_build_info` (1 match for `atc_build_info`) and `PgMetrics::register` (16 matches — one per distinct metric name; the labeled metrics `atc_pg_write_failures_total` and `atc_pg_notify_emitted_total` each get one describe covering both label values, and `atc_pg_in_memory_drift_total` remains described even though no handle is cached for it).
- **AC3.** `cargo nextest run -p atc-server` passes — specifically the PG-metric integration suites in [Regression net](#regression-net) (`transactional_writes_tests`, `outbox_tests`, `metrics_broadcast_watermark_test`, `metrics_min_pending_seq_test`, `metrics_drain_*_test`, `metrics_outbox_lag_test`, `metrics_wake_coalesced_test`, `gap_healing`).
- **AC4.** `cargo clippy -p atc-server -- -D warnings` clean.
- **AC5.** `docs/architecture/metrics.md` contains a subsection naming the cached-handle convention, citing `PgMetrics`, and explaining `register_build_info`'s exception as "one-shot at startup" (not "metadata-only"). `scripts/check-docs-lefthook.sh` accepts the diff.
- **Failure cases:** A reintroduced inline `metrics::counter!()` in production code fails AC1's grep. A handle wired but never read fails clippy (`dead_code`). A handle cached before the recorder install binds to the no-op recorder; the affected metric falls flat to zero across the regression-net suites in [Regression net](#regression-net). A presence-based assertion left unconverted in Phase 3 trips at harness-startup time because `PgMetrics::register()` now installs a live observable that exports `0`.

## Documents to Update

| File | Change |
|---|---|
| `docs/architecture/metrics.md` | Add subsection "Cached handle convention" under "Metric and span authoring contract" stating that production emits in `atc-server` cache handles on `PgMetrics`; `describe_*!` and `register_build_info`'s one-shot `gauge.set(1.0)` are the only inline exceptions, with the rationale spelled out (describe is metadata-only; `register_build_info` is a real emit safe to leave inline only because it runs exactly once at startup). Cite `backend/crates/atc-server/src/metrics.rs` as canonical. |
| `docs/architecture/backend-server.md` | Touch only as required by `scripts/doc-mapping.sh` (which maps `pg.rs` and `listener.rs` to both docs). Cross-reference the new convention in one line from the persistence section; do not duplicate the convention text. The doc-staleness gate only checks whether the mapped paths appear in the changed-file list, so a one-line cross-reference satisfies it. |
| `backend/crates/atc-server/CLAUDE.md` | Update the `metrics` module description in the modules table: replace "OTel-emitted metric registration helpers (`register_build_info`, `register_pg_write_counters`, `register_listener_metrics`)" with "OTel-emitted metric registration: `register_build_info` (one-shot startup gauge) and `PgMetrics::register` (cached `Counter`/`Gauge`/`Histogram` handles)". |
| `scripts/doc-mapping.sh` | No change — `metrics.rs`, `pg.rs`, `listener.rs` mappings already cover the touched files. |

## Out of Scope

- The bench AC from issue #155 (per author direction; correctness-shaped defense in depth, not perf).
- Caching the `atc_build_info` one-shot gauge handle (one-shot at startup makes caching pure ceremony).
- Caching for test-only emit sites in `tests/integration/otel_init_test.rs` (per issue: "Test code keeps inline emits").
- `atc_pg_in_memory_drift_total` — described but never emitted; left untouched.
- Adding new metrics, renaming any metric, or changing any attribute/label semantics.

## Verification

End-to-end:
1. `just lint` and `just test` pass locally (`just lint` runs clippy + biome + rustfmt; `just test` runs backend `cargo nextest` plus frontend `vitest`).
2. `just dev` against a local PG (per `docs/architecture/deployment.md`): fire a synthetic webhook via `curl`; confirm Prometheus scrape (or OTel collector dump) shows `atc_pg_notify_emitted_total{kind="run"}` and `atc_pg_drain_rows_total` advancing on subsequent webhook deliveries.
3. Re-run each suite in [Regression net](#regression-net) on the final branch state: counter values, gauge swaps, and histogram observations match the pre-refactor baseline captured in Phase 1.
