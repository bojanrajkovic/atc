# Metric interpretation guide

Last verified: 2026-05-23

This guide is for an operator triaging an ATC alert or running ATC in production. It covers what a metric's value *means*, what sustained rates suggest, NaN-sentinel semantics, cross-replica aggregation guidance, and example queries. Canonical per-metric metadata (Name, Type, Attributes, Measures) lives in [`../architecture/metrics.md`](../architecture/metrics.md).

---

## NaN-sentinel meanings

Three observable-gauge metrics use a sentinel value that maps to `f64::NAN` in the collection callback. Grafana renders NaN as gaps in time-series panels — a gap means the sentinel is active, not a missing scrape.

### `atc_pg_min_pending_seq`

NaN means the drain is caught up: the `min_pending_seq` atomic holds `i64::MAX` (the value the drain swaps in once it has processed all seqs below the watermark). A non-NaN value is the lowest seq the drain knows is still pending below the watermark — the gap-healing pressure signal. NaN is preferred over rendering `i64::MAX as f64` (≈ 9.22e18), which would collapse the watermark signal on any dashboard that graphs both gauges together.

### `atc_pg_outbox_min_replica_watermark`

NaN (sentinel: `-1`) covers two states:
- **Just-started replica** — the heartbeat task has not run yet; the atomic initializes to `-1` and the first heartbeat tick overwrites it.
- **No live replicas recently** — either a cluster partition or a full shutdown; no replica has heartbeated within the staleness window.

All replicas should read the same value (the cluster-wide `MIN(broadcast_watermark)`) within the 30-second heartbeat skew. A replica that persistently diverges from the cluster majority has a stalled heartbeat task.

### `atc_pg_outbox_oldest_row_age_seconds`

NaN (sentinel: `-1`) means the outbox is empty — `MIN(inserted_at)` returned NULL. Under healthy steady state the value oscillates near the configured `outbox_retention` window and the rate-of-change tracks the sweep rate. A monotonically rising value past `outbox_retention` indicates the sweep is not deleting rows (verify `atc_pg_outbox_rows_deleted_total` rate; check for sub-floor retention misconfiguration or absent heartbeats).

---

## Parity vs transient write failures

`atc_pg_write_failures_total` has two `kind` values with distinct severity routing:

- **`kind="parity"`** — the PG UPSERT matched 0 rows because the WHERE predicate rejected the transition (PG's view of state diverged from the in-memory model). Parity rejections return HTTP 200 `{"status":"rejected"}` to GitHub and are **not** retried. A sustained nonzero parity rate means state-machine drift between PG and the in-memory model — treat this as page-worthy.
- **`kind="transient"`** — sqlx errors at `pool.begin()`, mid-transaction, or `tx.commit()`. Transient failures return HTTP 503 and **are** retried by GitHub's webhook delivery. A sustained nonzero transient rate means the database path is unhealthy.

A `WARN` log with `target_status` is emitted alongside every parity rejection to surface which status the rejected transition was targeting.

**Aggregation:** `sum by (kind) (rate(atc_pg_write_failures_total[5m]))` for severity routing; `max by (pod)` to localize a misbehaving replica.

---

## Listener health

### `atc_pg_notify_received_total`

Every replica's listener receives every NOTIFY (PG fans out to all sessions holding `LISTEN atc_outbox`), so the per-replica rate should track parity across replicas. A replica whose rate falls behind the others has a stuck or stalled listener.

Sqlx hides successful reconnects: a counter that briefly plateaus and then catches up is a normal reconnect. A counter that stops without resuming is a stuck listener.

**Aggregation:** `avg by (pod) (rate(...))` to verify parity across replicas; `min by (pod) (rate(...))` to flag a replica whose listener is stuck.

**Example PromQL:** `rate(atc_pg_notify_received_total[5m])`

### `atc_pg_listener_recv_errors_total`

Sqlx attempts to reconnect transparently on most listener errors; this counter only fires when the error escapes that retry loop. A nonzero rate over more than a single scrape window means the listener is repeatedly failing to recover — likely DSN or session-mode misconfiguration (see `backend-server.md` § "DSN session-mode contract").

**Aggregation:** `max by (pod) (rate(...))` — a single misbehaving replica is the actionable signal.

**Example PromQL:** `rate(atc_pg_listener_recv_errors_total[5m])`

---

## Drain health

### `atc_pg_drain_passes_total`

Heartbeat-only wakes (the 5-second readiness tick) do NOT increment this counter — only NOTIFY-driven passes count. A flat-zero rate during a period of nonzero `atc_pg_notify_received_total` indicates the drain task is wedged.

**Aggregation:** `rate(... [5m]) by (pod)` — verify that drain passes are running on every replica that is receiving NOTIFYs. Pair with `atc_pg_notify_received_total` for a "wake → drain" sanity check.

**Example PromQL:** `rate(atc_pg_drain_passes_total[5m])`

### `atc_pg_drain_rows_total`

Cluster-wide `rate(atc_pg_drain_rows_total)` summed across replicas should approximately equal `rate(atc_pg_notify_emitted_total)` × replica count over the same window (each replica's drain reads every committed row).

**Aggregation:** `sum by (pod)` per-replica; `sum without (pod, instance)` for cluster total.

**Example PromQL:** `rate(atc_pg_drain_rows_total[5m])`

### `atc_pg_drain_duplicate_skipped_total`

Nonzero rate is the gap-healing rescan signal: the drain re-fetched a range of seqs because a NOTIFY arrived for a seq below the local watermark, and dedup correctly suppressed re-broadcast. Brief nonzero values during reorder windows are normal. A sustained high rate means the drain is repeatedly rescanning the same range — indicates either backstop math drift or an upstream NOTIFY storm.

**Aggregation:** `max by (pod) (rate(...))` — sustained nonzero rate on any single replica is the actionable signal.

**Example PromQL:** `rate(atc_pg_drain_duplicate_skipped_total[5m])`

### `atc_pg_drain_unknown_kind_total`

The set of legal outbox `kind` values is fixed by a CHECK constraint, so this counter should be flat zero in any healthy deployment. A nonzero value is either a deploy-skew signal (an older replica writing a kind a newer replica does not understand) or a schema invariant violation. Alert on first observation.

**Aggregation:** `sum without (pod, instance) (increase(...))` over a multi-hour window for the alert rule.

**Example PromQL:** `increase(atc_pg_drain_unknown_kind_total[1h])`

### `atc_pg_wake_coalesced_total`

Counts NOTIFY arrivals observed by the listener while a drain pass was in flight. Sustained high values on any replica indicate a NOTIFY storm or slow drain.

**Aggregation:** `rate(... [5m]) by (pod)` then `max by (pod)`.

**Example PromQL:** `rate(atc_pg_wake_coalesced_total[5m])`

---

## Outbox lag — event age at broadcast, not drain lag

`atc_pg_outbox_lag_seconds` measures event age at the moment of broadcast — `clock.now() - row.inserted_at` — where `inserted_at DEFAULT now()` evaluates `transaction_timestamp()` (transaction start, not commit). The metric therefore includes writer-side transaction latency in addition to drain queueing time. Operators reading p99/p95 should interpret it as "how stale is a typical row at broadcast time," not "how far behind is my drain task."

**Aggregation:** `histogram_quantile(0.99, sum(rate(...)) by (le, pod))` then `max by (pod)` for alerting — the slowest replica is the operationally relevant signal because all replicas serve traffic.

**Example PromQL:** `histogram_quantile(0.99, sum(rate(atc_pg_outbox_lag_seconds_bucket[5m])) by (le, pod))`

---

## Drain pass duration

`atc_pg_drain_pass_duration_seconds` covers the full pass including all paginated batches. NOT recorded for heartbeat-only wakes.

**Aggregation:** `histogram_quantile(0.99, ...)` `by (pod)` for per-replica latency; `avg by (pod)` for trend tracking.

**Example PromQL:** `histogram_quantile(0.99, sum(rate(atc_pg_drain_pass_duration_seconds_bucket[5m])) by (le, pod))`

---

## Drain startup latency

`atc_pg_drain_startup_seconds` is one observation per process lifetime. Per the restart-recovery contract there is no historical replay; this measures startup readiness, not catch-up backlog.

**Aggregation:** `max by (pod)` over a window covering recent deploys (1h) — the slowest replica's startup is the operational signal.

**Example PromQL:** `histogram_quantile(0.99, sum(rate(atc_pg_drain_startup_seconds_bucket[1h])) by (le, pod))`

---

## Drain shutdown — tail at give-up, not lag at SIGTERM

`atc_pg_drain_shutdown_remaining_rows` counts outbox rows whose `seq` exceeds the replica's drain watermark at drain task exit. The count is taken at drain task exit, not at signal arrival: the webhook handler keeps writing outbox rows until axum's graceful shutdown drains in-flight requests, so rows committed during that window are included. Operators reading this metric are seeing "what was unscanned when the drain task gave up," not "how far behind the drain was when SIGTERM arrived."

The drain task completes the in-flight pass (if any) and stops, on the assumption that the unscanned tail rarely exceeds one drain pass (`DRAIN_BATCH_SIZE = 500`). Sustained observations above 500 should prompt either a drain-pass tuning review or a longer `terminationGracePeriodSeconds`.

When the post-shutdown count query fails or exceeds its 1-second timeout, the observation is skipped (logged as a warning) rather than recorded as zero — `_count` only advances on successful observations. A flat zero on a pod that recently restarted indicates the count query failed at shutdown; check the application log for warnings.

**Aggregation:** `histogram_quantile(0.99, ...)` `by (pod)` over a multi-deploy window (e.g. 24h) — the slowest replica's tail at shutdown is the actionable signal. `max by (pod) (rate(atc_pg_drain_shutdown_remaining_rows_count[24h]))` confirms each replica is recording observations across rollouts.

**Example PromQL:** `histogram_quantile(0.99, sum(rate(atc_pg_drain_shutdown_remaining_rows_bucket[24h])) by (le, pod))`

---

## Broadcast watermark and cluster-laggiest-replica signal

`atc_pg_broadcast_watermark` is the commit-order cursor read by `state_handler` as `lastSeq` in PG mode. Display per pod; for a single cluster-wide "laggiest replica" series, use `min(atc_pg_broadcast_watermark)` (or equivalently `min without (pod, instance)`).

**Example PromQL:** `atc_pg_broadcast_watermark`

Display alongside `atc_pg_min_pending_seq`. Filter NaN with `... unless on() (atc_pg_min_pending_seq != atc_pg_min_pending_seq)` if needed.

---

## Outbox retention headroom

`atc_pg_outbox_rows_deleted_total` counts rows deleted by the retention sweep under `FOR UPDATE SKIP LOCKED` semantics — concurrent sweepers on other replicas account for disjoint candidate subsets, so the per-replica counter is the per-replica share, not a cluster-wide tally.

Healthy at steady state: `rate(...)` ≈ outbox write rate divided by replica count. A sustained-zero rate after at least one full retention window indicates either the sweep predicate is rejecting everything (sub-floor retention misconfigured, no fresh heartbeats — verify `atc_pg_outbox_min_replica_watermark`) or the outbox is not growing (no incoming webhooks).

**Aggregation:** `sum without (pod, instance) (rate(atc_pg_outbox_rows_deleted_total[5m]))` for cluster-wide rate; `rate(atc_pg_outbox_rows_deleted_total[5m])` per pod to compare contention shares.

**Example PromQL:** `sum without (pod, instance) (rate(atc_pg_outbox_rows_deleted_total[5m]))`

`atc_pg_outbox_min_replica_watermark` is refreshed every 30 s by the outbox heartbeat task (coarse-grained relative to OTel collection cadence). All replicas should observe the same value within the heartbeat skew; divergence surfaces a stalled heartbeat task.

**Aggregation:** `min without (pod, instance) (atc_pg_outbox_min_replica_watermark)` for the cluster-wide signal; per-pod comparison surfaces stalled replicas.

**Example PromQL:** `min without (pod, instance) (atc_pg_outbox_min_replica_watermark)` (Grafana renders NaN as gaps)

---

## Config reload failures

`atc_config_reload_total` distinguishes `result="success"` (`reason="applied"` or `reason="noop"`) from `result="failure"` (`reason="read"` | `"parse"` | `"validate"`). A sustained non-zero `reason="failure"` rate indicates the operator's most-recent YAML edit is invalid and the cluster is running on the previous good config.

**Aggregation:** `sum without (pod, instance) (rate(atc_config_reload_total[5m]))` for cluster-wide reload rate; per-reason breakdown surfaces failure spikes.

**Example PromQL:** `sum by (reason) (rate(atc_config_reload_total[5m]))`

---

## Config runner pool count

`atc_config_runner_pools` reflects the startup-loaded count until the first applied reload, then tracks the latest applied reload's pool count. All replicas mount the same ConfigMap so values should match within the kubelet sync window; divergence (~60 s skew) is normal during a rolling ConfigMap update.

**Aggregation:** `max without (pod, instance) (atc_config_runner_pools)` for the cluster-wide pool count; per-pod divergence during a rolling reload is expected.

**Example PromQL:** `max without (pod, instance) (atc_config_runner_pools)`

---

## WebSocket connection counts

`atc_ws_connections_active` counts in-flight `handle_socket` tasks per replica. Cluster-wide value is the sum across pods.

**Aggregation:** `sum without (pod, instance) (atc_ws_connections_active)`.

**Example PromQL:** `sum without (pod, instance) (atc_ws_connections_active)`

---

## WebSocket lagged evictions

`atc_ws_lagged_evictions_total` counts clients force-disconnected because the bounded broadcast buffer (capacity 256) overflowed. A sustained nonzero rate means the broadcast buffer is undersized for current traffic or a specific client is stalled.

Per-channel interpretation:
- **`channel="config"`** — suspicious, because operator-config reloads are low-volume; a stalled config receiver is the likely cause.
- **`channel="committed"`** — indicates a slow client under high webhook traffic.

**Aggregation:** `sum by (channel) (rate(atc_ws_lagged_evictions_total[5m]))` for per-channel eviction rate.

**Example PromQL:** `sum by (channel) (rate(atc_ws_lagged_evictions_total[5m]))`

---

## Process metrics — dashboard migration note

The `opentelemetry-system-metrics` observer emits a different surface from the prior `metrics_process` exposition. The prior recorder emitted `process_cpu_seconds_total`, `process_resident_memory_bytes`, `process_start_time_seconds`, `process_open_fds`, `process_max_fds`, and `process_threads`. Dashboards that filtered on those names need to be updated. ATC's bundled Grafana dashboard (`deploy/helm/atc/dashboards/atc-overview.json`) covers the full `process_*`, `http_*`, `atc_pg_*`, `atc_config_*`, and `atc_build_info` surface.

Operators relying on host- or container-level fd / start-time metrics should source them from the node exporter or container runtime sidecar instead.

The `process_cpu_usage` / `process_cpu_utilization` naming is inverted in `opentelemetry-system-metrics 0.31.0` (see crate source `src/lib.rs:131,214`): the Rust binding named `process_cpu_utilization` records CPU usage with attributes, and the binding named `process_cpu_usage` records CPU utilization without attributes. Dashboard queries that want per-process CPU with pod attribution should use `process_cpu_usage`, not `process_cpu_utilization`.
