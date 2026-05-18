# Issue #64 — Modernize Grafana dashboard + ship it via Helm with operator-toggle auto-discovery

## Context

The standalone dashboard at `deploy/grafana/atc-postgres-overview.json` ships only six PG-path panels (drain p99, outbox lag, watermark, etc.), but ATC's actual metric surface — documented in `docs/architecture/metrics.md` and confirmed by surveying emission sites in `backend/crates/atc-server/src/{metrics,otel,config_watcher}.rs` and `backend/crates/atc-store-pg/src/metrics.rs` — is much broader:

- **HTTP:** `http_server_request_duration_seconds` histogram from `axum-otel-metrics` 0.13.0 with `http.request.method`, `http.response.status_code`, `http.route`, `server.address` attributes (axum-otel-metrics records `server.address`, not `url.scheme`, on the duration histogram — see `crate axum-otel-metrics 0.13.0/src/lib.rs:447`).
- **Build info:** `atc_build_info` gauge with six labels (`version`, `git_describe`, `git_sha`, `rustc_version`, `build_timestamp`, `target_triple`).
- **Process (OTel `opentelemetry-system-metrics` 0.31.0):** `process_cpu_usage` (per-process attributes — `process.pid`, `process.executable.name`, `process.executable.path`, `process.command` — value is `raw_cpu / core_count` normalized 0..100% of host capacity), `process_cpu_utilization` (NO attributes, raw `cpu_usage` summed across cores 0..N*100%), `process_memory_usage`, `process_memory_virtual`, `process_disk_io` (direction=read|write). **Note:** the upstream crate has its variable names *inverted* from the metric names they're bound to (`process_cpu_utilization` Rust binding writes `PROCESS_CPU_USAGE` metric and vice versa — see `opentelemetry-system-metrics-0.31.0/src/lib.rs:131,214`). `docs/architecture/metrics.md` § Process collector documents the inverse and needs a correction as part of this PR.
- **Config:** `atc_config_reload_total{result,reason}`, `atc_config_runner_pools` gauge.
- **PG path (17 metrics):** writer (`atc_pg_write_failures_total{kind}`, `atc_pg_notify_emitted_total{kind}`, `atc_pg_in_memory_drift_total`), listener (`atc_pg_notify_received_total`, `atc_pg_listener_recv_errors_total`), drain (`atc_pg_drain_passes_total`, `atc_pg_drain_rows_total`, `atc_pg_drain_duplicate_skipped_total`, `atc_pg_drain_unknown_kind_total`, `atc_pg_wake_coalesced_total`, `atc_pg_outbox_lag_seconds`, `atc_pg_drain_pass_duration_seconds`, `atc_pg_drain_startup_seconds`, `atc_pg_drain_shutdown_remaining_rows`), watermarks/retention (`atc_pg_broadcast_watermark`, `atc_pg_min_pending_seq`, `atc_pg_outbox_min_replica_watermark`, `atc_pg_outbox_oldest_row_age_seconds`, `atc_pg_outbox_rows_deleted_total`).

No websocket, tokio-runtime, or sqlx-pool metrics today — they're explicitly out of scope.

Issue #64 sketches an opt-in Helm ConfigMap with the kube-prometheus-stack sidecar discovery label (`grafana_dashboard: "1"`). The user's broader ask: design a *really nice* dashboard that uses the latest Grafana 11 patterns AND supports both major auto-discovery mechanisms — the kiwigrid/k8s-sidecar AND grafana-operator v5's `GrafanaDashboard` CR.

Research confirms:

- **Sidecar (kiwigrid/k8s-sidecar via kube-prometheus-stack):** discovery is label-based (`grafana_dashboard: "1"`), folder via annotation (`grafana_folder: "<Name>"`), namespace controlled by upstream env `NAMESPACE` (Grafana's helm chart exposes the same knob as `sidecar.dashboards.searchNamespace`; default = sidecar's own ns).
- **grafana-operator v5:** CR is `GrafanaDashboard` on `grafana.integreatly.org/v1beta1`; spec supports inline JSON, `configMapRef`, `url`, or `gzipJson`; `instanceSelector` picks the target Grafana(s); `folderRef` points at a `GrafanaFolder` CR. The CR can read the SAME ConfigMap the sidecar discovers — one source, two paths.
- **Datasource portability:** the existing dashboard hardcodes `"uid": "Prometheus"` which is a deploy-local accident. The Grafana-recommended pattern is a `datasource` template variable of `type: datasource` with `query: prometheus`; every panel references `"uid": "${datasource}"` and Grafana auto-resolves against any Prometheus datasource in the org.

## Definition of Done

1. A modernized dashboard JSON exists at **`deploy/helm/atc/dashboards/atc-overview.json`** (canonical home moved inside the chart so `helm package` picks it up without symlink trickery). It uses Grafana 11 panel types, an organized row layout, and covers every metric the backend emits today.
2. Panel datasource references use a `${datasource}` template variable — portable across operator stacks.
3. An opt-in Helm template (`deploy/helm/atc/templates/grafana-dashboard.yaml`) renders a ConfigMap labeled for kube-prometheus-stack sidecar discovery when `grafanaDashboard.enabled: true`. Default OFF.
4. A nested sub-toggle (`grafanaDashboard.grafanaOperator.enabled: true`) additionally renders a `grafana.integreatly.org/v1beta1 GrafanaDashboard` CR that references the same ConfigMap.
5. `values.schema.json` declares the new keys with `additionalProperties: false`.
6. Helm-unittest suite covers: disabled-default omits both; enabled emits ConfigMap with correct labels/annotations; namespace override flows through; operator-CR-enabled emits both resources with cross-references; folder annotation handling.
7. `docs/architecture/deployment.md` gains a "Grafana dashboard" subsection; `docs/architecture/metrics.md` updates the path reference; `deploy/helm/atc/CLAUDE.md` adds a contract bullet; `scripts/doc-mapping.sh` adds a mapping line.
8. The old `deploy/grafana/atc-postgres-overview.json` is removed; `deploy/grafana/README.md` redirects readers to the canonical chart-internal path.
9. A PR is opened against `main`, with CI green, but NOT merged (the user holds the merge approval).

## Locked Decisions

- **Single canonical dashboard home, inside the chart.** No symlink (`helm package` semantics are inconsistent across tooling — Helm's `Files.Glob` walker does not consistently follow symlinks), no duplicated file. The old `deploy/grafana/` directory becomes a README redirect. Source: pattern set by the existing `deploy/helm/atc/templates/configmap.yaml` which embeds runner-pool YAML inline via `.Files.Get`-equivalent expansion (`toYaml .Values.runnerPools | indent 6`).
- **Datasource via template variable, not Helm string substitution.** The issue's sketch (`dashboard.datasource: Prometheus` rewritten at template-render time) is rejected because (a) it forces Helm to do JSON surgery on `.Files.Get` output, (b) it makes the dashboard non-portable as a standalone Grafana-UI import, (c) every operator with a non-default datasource name would need to know to set the chart value. Source: issue #64 body (open-question section); the recommended-pattern reasoning lands in `deploy/helm/atc/CLAUDE.md` as a new contract bullet.
- **One ConfigMap, optionally consumed by a CR.** When `grafanaOperator.enabled: true`, the CR's `spec.configMapRef` points at the ConfigMap the sidecar would also discover. No inline-JSON duplication. Source: grafana-operator v5 docs (`spec.configMapRef` is a documented field on `GrafanaDashboard` in `grafana.integreatly.org/v1beta1`); pattern mirrors `templates/configmap.yaml` (existing chart-emitted ConfigMap that downstream resources consume).
- **Default off (`grafanaDashboard.enabled: false`).** Per issue #64's open-question resolution: provisioning is an operator action, not a chart default. Source: `deploy/helm/atc/CLAUDE.md:24` (the operator-toggle gating idiom — every optional resource ships off by default) and the feedback memory `feedback_no_chart_render_guard_for_operator_toggleable_features.md`.
- **Histogram query form:** use the **native histogram** form — `histogram_quantile(0.99, sum by (label) (rate(name[5m])))`, no `_bucket` suffix, no `le` grouping. The OTel SDK uses `Aggregation::Base2ExponentialHistogram` (see `backend/crates/atc-server/src/otel.rs:169` and `:199`), and the OTLP→Prometheus translator surfaces these as Prometheus native histograms when the storage supports them (Prometheus 2.40+, Mimir, the grafana/otel-lgtm bundle ATC's own dev stack runs). `docs/architecture/metrics.md` § Histogram aggregation needs correction in this PR — the prior claim that classic-form queries "continue to work" against native histograms is wrong (native histograms have no `_bucket` series; the classic query returns empty). Operators running collectors that emit only classic histograms must translate panel queries to the classic form: `histogram_quantile(0.99, sum by (le, label) (rate(name_bucket[5m])))`. Document this in the dashboard `description` and the deployment.md "Histogram-aggregation assumption" subsection. (Inverted mid-PR from the initial classic-form draft per operator direction — native is where the ecosystem is, classic is the legacy escape hatch.)
- **`atc_build_info` rendered as a stat panel with all-text override** showing version + git_sha — operators want to read it at a glance, not query it via PromQL. Source: `docs/architecture/metrics.md` § atc_build_info (the documented intent is "operator-facing identifier").
- **No alert rules in this PR.** Alerts are out of scope; the dashboard is the deliverable. Issue text doesn't ask for alerts, and `docs/architecture/metrics.md` doesn't have a canonical PromQL alert ruleset to ship from. Source: issue #64 body (the deliverable is a Helm-bundled dashboard; alerts are not mentioned).
- **PR test plan goes in the first PR comment, not the body.** Source: `CONTRIBUTING.md` § Pull Requests (test-plan placement convention).
- **TDD ordering: failing helm-unittest suite first, then template + JSON.** Source: `docs/planning-workflow.md` § Implementation Phases ("Step 1 should be 'write failing tests'; Step 2 should be 'make them pass'") and `docs/implementation-guidance.md` § TDD.

## Architecture

### Dashboard layout (Grafana 11.x, `schemaVersion: 39`)

One JSON file, organized into collapsible rows. Each row is a Grafana `row` panel; collapsing them gives operators a tidy default view.

**Row 1 — Overview**
- `atc_build_info` stat panel (one row per pod; transformations: filterByName regex on labels, organize fields to show pod + version + git_sha + git_describe).
- HTTP request rate stat: `sum(rate(http_server_request_duration_seconds_count{pod=~"$pod"}[$__rate_interval]))`.
- HTTP 5xx error rate stat with red threshold: `sum(rate(http_server_request_duration_seconds_count{pod=~"$pod",http_response_status_code=~"5.."}[$__rate_interval]))`.
- HTTP p99 latency stat: `histogram_quantile(0.99, sum(rate(http_server_request_duration_seconds_bucket{pod=~"$pod"}[$__rate_interval])) by (le))`.
- `atc_config_runner_pools` gauge with text override.

**Row 2 — HTTP traffic**
- Request rate by route+method timeseries: `sum by (http_route, http_request_method) (rate(http_server_request_duration_seconds_count{pod=~"$pod"}[$__rate_interval]))`.
- Status code distribution stacked area: `sum by (http_response_status_code) (rate(http_server_request_duration_seconds_count{pod=~"$pod"}[$__rate_interval]))`.
- p50/p95/p99 latency by route (three series per route): `histogram_quantile(0.95, sum by (le, http_route) (rate(http_server_request_duration_seconds_bucket{pod=~"$pod"}[$__rate_interval])))`.
- Error rate by route: same shape, filtered on `http_response_status_code=~"5.."`.
- HTTP duration heatmap (across all routes): the histogram itself as a heatmap panel.

**Row 3 — Webhook ingestion**
- `atc_pg_notify_emitted_total` rate by kind (run/job): timeseries.
- `atc_pg_write_failures_total` rate by kind (parity/transient): timeseries with red threshold above 0.
- `atc_pg_in_memory_drift_total` rate: timeseries with red threshold above 0 (sustained nonzero = page).

**Row 4 — Drain pipeline**
- NOTIFY-vs-pass cross-check: `rate(atc_pg_notify_received_total[$__rate_interval])` and `rate(atc_pg_drain_passes_total[$__rate_interval])` per pod, overlaid.
- Drain pass duration p99: `histogram_quantile(0.99, sum(rate(atc_pg_drain_pass_duration_seconds_bucket{pod=~"$pod"}[$__rate_interval])) by (le, pod))`.
- Outbox lag p99 (event age at broadcast): `histogram_quantile(0.99, sum(rate(atc_pg_outbox_lag_seconds_bucket{pod=~"$pod"}[$__rate_interval])) by (le, pod))`. Yellow at 1s, red at 5s.
- Wake-coalesced rate: `rate(atc_pg_wake_coalesced_total{pod=~"$pod"}[$__rate_interval])`.
- Drain throughput: `rate(atc_pg_drain_rows_total[$__rate_interval])` and `rate(atc_pg_drain_passes_total[$__rate_interval])` overlaid.
- Drain duplicate-skipped rate: `rate(atc_pg_drain_duplicate_skipped_total[$__rate_interval])`.
- Drain unknown-kind increase (1h, alert-style): `increase(atc_pg_drain_unknown_kind_total[1h])`, threshold red above 0.
- Listener recv errors: `rate(atc_pg_listener_recv_errors_total[$__rate_interval])`.

**Row 5 — Watermarks & retention**
- Per-pod watermark + min_pending_seq overlay (existing panel, kept verbatim with dashed line override for min_pending_seq).
- Cluster floor: `min without (pod, instance) (atc_pg_outbox_min_replica_watermark)`.
- Outbox oldest row age: `max without (pod, instance) (atc_pg_outbox_oldest_row_age_seconds)`. Annotate retention horizon as a yellow line based on a `$retention_seconds` constant variable defaulted to 604800 (7d).
- Retention sweep rate: `sum without (pod, instance) (rate(atc_pg_outbox_rows_deleted_total[$__rate_interval]))`.

**Row 6 — Startup & shutdown (low-frequency)**
- Drain startup p99 (1h window): `histogram_quantile(0.99, sum(rate(atc_pg_drain_startup_seconds_bucket{pod=~"$pod"}[1h])) by (le, pod))`.
- Drain shutdown remaining rows p99 (24h window).

**Row 7 — Process**
- `max by (pod) (process_cpu_usage{pod=~"$pod"})` timeseries — uses `process_cpu_usage` because that's the per-process metric (with `process.pid` etc. attributes — see upstream crate's inverted variable binding). Aggregates away the per-process labels so the legend is one series per pod. Unit: percent (0..100% of host capacity).
- `max by (pod) (process_memory_usage{pod=~"$pod"})` timeseries (bytes unit, IEC).
- `max by (pod) (process_memory_virtual{pod=~"$pod"})` timeseries.
- `sum by (pod, direction) (rate(process_disk_io{pod=~"$pod"}[$__rate_interval]))` timeseries split by `direction`.

**Row 8 — Config reloads**
- `sum by (result, reason) (rate(atc_config_reload_total{pod=~"$pod"}[$__rate_interval]))`.
- `atc_config_runner_pools` over time (gauge as line — surfaces operator edits).

`uid`: `atc-overview` (was `atc-postgres-overview` — rename signals broader scope; old uid no longer ships).
`title`: `ATC — Overview`.
`tags`: `["atc"]`.
`refresh: 30s`, `time.from: now-1h`.

**Template variables:**
- `datasource` — type: `datasource`, query: `prometheus`, no `label_values()` query, default: first match. Operators with one Prometheus get auto-resolution; multiple get a picker.
- `pod` — type: `query`, query: `label_values(atc_build_info, pod)` (broader than `atc_pg_drain_passes_total` from the old dashboard — works in in-memory mode too where no PG metric exists). `includeAll: true`, `multi: true`.
- `retention_seconds` — type: `constant`, default `604800`. Used by the retention-horizon overlay panel.

### Helm template — `deploy/helm/atc/templates/grafana-dashboard.yaml`

```gotmpl
{{- if .Values.grafanaDashboard.enabled -}}
apiVersion: v1
kind: ConfigMap
metadata:
  name: {{ include "atc.fullname" . }}-dashboard
  {{- with .Values.grafanaDashboard.namespace }}
  namespace: {{ . }}
  {{- end }}
  labels:
    {{- include "atc.labels" . | nindent 4 }}
    {{- with .Values.grafanaDashboard.labels }}
    {{- toYaml . | nindent 4 }}
    {{- end }}
  {{- with .Values.grafanaDashboard.annotations }}
  annotations:
    {{- toYaml . | nindent 4 }}
  {{- end }}
data:
  atc-overview.json: |-
{{ .Files.Get "dashboards/atc-overview.json" | indent 4 }}
{{ if .Values.grafanaDashboard.grafanaOperator.enabled }}
---
apiVersion: grafana.integreatly.org/v1beta1
kind: GrafanaDashboard
metadata:
  name: {{ include "atc.fullname" . }}-dashboard
  {{- with .Values.grafanaDashboard.namespace }}
  namespace: {{ . }}
  {{- end }}
  labels:
    {{- include "atc.labels" . | nindent 4 }}
spec:
  instanceSelector:
    {{- toYaml .Values.grafanaDashboard.grafanaOperator.instanceSelector | nindent 4 }}
  {{- with .Values.grafanaDashboard.grafanaOperator.folderRef }}
  folderRef: {{ . | quote }}
  {{- end }}
  {{- with .Values.grafanaDashboard.grafanaOperator.resyncPeriod }}
  resyncPeriod: {{ . | quote }}
  {{- end }}
  configMapRef:
    name: {{ include "atc.fullname" . }}-dashboard
    key: atc-overview.json
{{- end }}
{{- end }}
```

### `values.yaml` shape

```yaml
# Grafana dashboard auto-discovery. Default off — provisioning a dashboard is
# an operator decision, not a chart default. Two discovery paths are supported
# from one ConfigMap source; enable whichever (or both) matches your stack.
grafanaDashboard:
  # -- Render the dashboard ConfigMap. The bundled JSON covers every metric the
  # backend emits today (HTTP, process, PG-path, config) and uses a Grafana
  # template variable for the Prometheus datasource so it works regardless of
  # what the operator named their datasource.
  enabled: false

  # -- Namespace for the ConfigMap. Empty (default) renders in the release
  # namespace. Set when the Grafana sidecar discovers dashboards only from a
  # specific namespace (kube-prometheus-stack default is its own ns).
  namespace: ""

  # -- Discovery labels for the ConfigMap. `grafana_dashboard: "1"` is the
  # kiwigrid/k8s-sidecar convention (used by kube-prometheus-stack). Override
  # if your stack uses a different label key/value.
  labels:
    grafana_dashboard: "1"

  # -- Annotations for the ConfigMap. `grafana_folder: "ATC"` places the
  # dashboard in a named Grafana folder; `k8s-sidecar-target-directory: "/..."`
  # overrides the on-disk path.
  annotations: {}

  grafanaOperator:
    # -- Additionally render a grafana-operator v5 GrafanaDashboard CR
    # (grafana.integreatly.org/v1beta1) that references the ConfigMap above.
    # Requires the grafana-operator CRDs installed; the chart does not bundle
    # them.
    enabled: false
    # -- instanceSelector for the GrafanaDashboard CR — labels matching one or
    # more Grafana CRs that should mount this dashboard.
    instanceSelector:
      matchLabels:
        dashboards: "grafana"
    # -- Optional folder reference (name of a GrafanaFolder CR in the same
    # namespace). Empty omits the field; the dashboard lands in the default
    # folder.
    folderRef: ""
    # -- Optional resyncPeriod (Go duration string, e.g. "5m"). Empty omits the
    # field; the operator uses its default.
    resyncPeriod: ""
```

### `values.schema.json` addition

```json
"grafanaDashboard": {
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "enabled":   { "type": "boolean" },
    "namespace": { "type": "string" },
    "labels":    { "type": "object", "additionalProperties": { "type": "string" } },
    "annotations": { "type": "object", "additionalProperties": { "type": "string" } },
    "grafanaOperator": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "enabled": { "type": "boolean" },
        "instanceSelector": {
          "type": "object",
          "description": "Pass-through to GrafanaDashboard.spec.instanceSelector (Kubernetes LabelSelector shape). Only matchLabels / matchExpressions are validated structurally; the chart treats this as raw pass-through so the operator can use any LabelSelector form the grafana-operator CR accepts.",
          "additionalProperties": false,
          "properties": {
            "matchLabels":      { "type": "object", "additionalProperties": { "type": "string" } },
            "matchExpressions": { "type": "array", "items": { "type": "object" } }
          }
        },
        "folderRef":    { "type": "string" },
        "resyncPeriod": { "type": "string" }
      }
    }
  }
}
```

## Implementation Steps

### Branch and plan commit

Create `feat/issue-64-grafana-dashboard-helm` from `main`. Copy this plan into `docs/design-plans/2026-05-18-issue-64-grafana-dashboard-helm-bundling.md`. Commit as `docs: add design plan for issue #64 Grafana dashboard helm bundling`.

### Helm-unittest suite (red)

Write `deploy/helm/atc/tests/unit/grafana-dashboard.yaml` BEFORE the template exists. Per `docs/planning-workflow.md` § Implementation Phases and `docs/implementation-guidance.md` § TDD ordering, the suite is RED until the template + values exist. Cases:

- `omits both resources when grafanaDashboard.enabled is false` — `hasDocuments: { count: 0 }` against `grafana-dashboard.yaml`.
- `renders a ConfigMap with the sidecar label when enabled` — `enabled: true`, default labels, assert `kind: ConfigMap`, `metadata.labels.grafana_dashboard == "1"`, `data."atc-overview.json"` non-empty and contains `"uid": "atc-overview"`.
- `applies the namespace override` — `namespace: monitoring`, assert `metadata.namespace == "monitoring"`.
- `merges custom labels and annotations` — set both, assert presence.
- `renders the GrafanaDashboard CR alongside the ConfigMap when grafanaOperator is enabled` — `enabled: true`, `grafanaOperator.enabled: true`, assert two documents (`hasDocuments: { count: 2 }`); the CR's `spec.configMapRef.name` matches the ConfigMap name; `spec.configMapRef.key == "atc-overview.json"`; `instanceSelector` flows through.
- `omits folderRef and resyncPeriod when empty strings` — defaults assert the fields are absent in the CR spec.

### Kubeconform fixture

Add `deploy/helm/atc/tests/values-grafana-dashboard.yaml` enabling both `enabled: true` and `grafanaOperator.enabled: true`. This file feeds the existing `just helm-check` matrix (which iterates over every `tests/values-*.yaml` via `scripts/helm-kubeconform-one.sh` and `scripts/helm-kubeconform-batch.sh`). The Datree CRDs catalog has `grafana.integreatly.org/grafanadashboard_v1beta1.json`, so the `GrafanaDashboard` CR validates without needing `-ignore-missing-schemas`.

### Helm template + values + schema (green)

- Create `deploy/helm/atc/templates/grafana-dashboard.yaml` with the structure shown above (using `{{ if }}` without leading-dash trim on the second `if` so the document separator survives a JSON file without a trailing newline; also ensure the JSON file does end with a newline as a belt-and-suspenders).
- Append `grafanaDashboard:` block to `deploy/helm/atc/values.yaml`.
- Extend `deploy/helm/atc/values.schema.json` with the new top-level key.

### Dashboard JSON

Author `deploy/helm/atc/dashboards/atc-overview.json` with the eight-row layout above. Use `schemaVersion: 39`, `refresh: 30s`, `time.from: now-1h`. Template variables: `datasource`, `pod`, `retention_seconds`. Tags `["atc"]`. UID `atc-overview`. Title `ATC — Overview`. File MUST end with a `\n`.

Remove `deploy/grafana/atc-postgres-overview.json`. Replace `deploy/grafana/` with a `README.md` redirect pointing at `deploy/helm/atc/dashboards/atc-overview.json` and noting the standalone-import workflow (download the JSON, import via Grafana UI).

### Doc updates

- `docs/architecture/deployment.md` — add a "Grafana dashboard" subsection. Cover: the toggle, the dual sidecar/operator paths, the canonical JSON path, the datasource template variable, the default-off rationale, and operator-side prerequisites (kube-prometheus-stack OR grafana-operator).
- `docs/architecture/metrics.md` — update the parenthetical at line 191 from `deploy/grafana/atc-postgres-overview.json` to `deploy/helm/atc/dashboards/atc-overview.json` and note that the dashboard now covers the full metric surface. Correct the Process collector section — `process_cpu_usage` carries the per-process attributes and `process_cpu_utilization` does not (the table rows are swapped). Tighten the Histogram aggregation note — native-histogram emission means classic `_bucket` queries return empty; the bundled dashboard targets classic emission (the default OTel→Prometheus translator path).
- `deploy/helm/atc/CLAUDE.md` — add a contract bullet for `grafanaDashboard.*` gating, mirroring the existing per-feature contract style.
- `scripts/doc-mapping.sh` — no change. The existing `deploy/helm/atc/*` catch-all (line 122) already covers `deploy/helm/atc/dashboards/*`.

### Lint & verify

- `just helm-lint && just helm-unittest && just helm-check` — `helm-check` runs the matrix of `scripts/helm-kubeconform-one.sh` across every kube-version × `tests/values-*.yaml`, including the new dashboard fixture.
- `python3 -m json.tool deploy/helm/atc/dashboards/atc-overview.json > /dev/null` — JSON validity sanity check.

### PR

Title: `feat(helm): bundle modernized Grafana dashboard with sidecar + operator discovery`. Body: short summary of the deliverable. The test plan goes in the **first PR comment**, not the body, per `CONTRIBUTING.md` § Pull Requests. Links issue #64. Trailer: `Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>`. **Do not merge** — the user holds the merge approval.

## Acceptance Criteria

- **AC1** — `just helm-lint`, `just helm-unittest`, and `just helm-check` all pass locally on the branch.
- **AC2** — `just helm-unittest` reports the new `grafana-dashboard.yaml` suite green; every case enumerated in the Helm-unittest suite step is asserted explicitly via helm-unittest's `equal`/`contains`/`matchRegex`/`isKind` assertions (not via shell greps that can drift).
- **AC3** — `scripts/helm-kubeconform-one.sh` with the new `tests/values-grafana-dashboard.yaml` fixture and the chart's pinned kube-version validates both rendered documents (ConfigMap + GrafanaDashboard CR) against the Datree CRDs catalog without `-ignore-missing-schemas`.
- **AC4** — `python3 -m json.tool deploy/helm/atc/dashboards/atc-overview.json > /dev/null` exits 0.
- **AC5** — `git grep -n '"uid"[[:space:]]*:[[:space:]]*"Prometheus"' deploy/helm/atc/dashboards/atc-overview.json` returns zero hits (the dashboard uses `${datasource}` everywhere; the literal-uid pattern is the deploy-local accident we're removing).
- **AC6** — `scripts/check-docs-lefthook.sh` doesn't block on push: docs that map to backend metric files (`metrics.md`) are touched alongside their source counterparts in this PR.
- **AC7** — CI green on the PR. PR is open against `main`, NOT merged.
- **AC8** — `git grep -nE 'deploy/grafana/atc-postgres-overview\.json' -- ':!docs/design-plans/' ':!deploy/grafana/README.md'` returns zero hits after this change (the old standalone-path string is gone from shipped docs and configs; the design plan and the redirect README are intentionally excluded — the plan is an immutable design artifact and the redirect README necessarily references the old name).

## Documents to Update

- `docs/architecture/deployment.md` — new "Grafana dashboard" subsection.
- `docs/architecture/metrics.md` — three changes: (a) path reference at line ~191 from old standalone JSON to new chart-internal path; (b) Process collector table — swap the attribute rows for `process_cpu_usage` and `process_cpu_utilization` to reflect the upstream crate's actual binding; (c) Histogram aggregation note — tighten the native-histogram claim (classic `_bucket` queries return empty against native-only emission; the bundled dashboard targets classic-histogram emission, which is the default OTel→Prometheus translator path).
- `deploy/helm/atc/CLAUDE.md` — new contract bullet for `grafanaDashboard.*` gating.
- `scripts/doc-mapping.sh` — no change. The existing `deploy/helm/atc/*` catch-all covers `dashboards/*`.

## Out of Scope

- Prometheus alert rules / PrometheusRule template — separate issue, no ask in #64.
- ServiceMonitor template — was removed in chart 0.2 by design; operators wire scrape config externally.
- `spec.gzipJson` packaging for the CR — current dashboard is well under 100 KiB; `configMapRef` is enough.
- Multi-dashboard split (one per domain) — single overview is sufficient for the current surface; future split can add a `dashboards/` directory with multiple files and a chart loop.
- Frontend-side metrics — none emitted today.
- Adding tokio-runtime / sqlx-pool metrics so we have something to dashboard — separate scope, would require backend changes.

## Glossary

- **Sidecar dashboard discovery** — pattern where a Grafana pod runs `kiwigrid/k8s-sidecar` alongside it, watching for ConfigMaps with a configured label and provisioning their `data` keys as dashboards into Grafana. Used by `kube-prometheus-stack`.
- **grafana-operator v5** — Kubernetes operator (`grafana-operator/grafana-operator`) that manages Grafana instances and dashboards via CRs (`Grafana`, `GrafanaDashboard`, `GrafanaFolder`, `GrafanaDatasource`). API group `grafana.integreatly.org/v1beta1`.
- **Datasource template variable** — Grafana variable of type `datasource` that lets a dashboard reference its datasource by a token (`${datasource}`) Grafana resolves against datasources matching the variable's `query` filter at view time.
