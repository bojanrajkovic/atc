# Deployment — Architecture

Last verified: 2026-05-18 (auth section + breaking-change-callout cleanup)

## Purpose

The ATC Helm chart (`deploy/helm/atc/`) packages the ATC server for deployment to any
conformant Kubernetes cluster. It produces a single mandatory Deployment backed by a
ClusterIP Service and a dedicated ServiceAccount. Optional resources (Ingress, HTTPRoute,
NetworkPolicy, `helm test` hook) are each gated behind independent values flags that
default to `false`. OpenTelemetry export is configured via the `otel.*` values block —
also default-disabled — which injects spec-standard `OTEL_*` env vars into the container
when enabled.

The chart is published via two parallel channels on the tag-triggered release workflow,
alongside the container image and binary artifacts: an OCI artifact at
`oci://ghcr.io/bojanrajkovic/charts/atc` (Sigstore-attested), and a classic HTTP Helm
repo on GitHub Pages at `https://bojanrajkovic.github.io/atc/charts` (recommended for
consumers without GHCR authentication).

## Key Decisions

**Decision:** Restricted Pod Security Standards by default, overridable via values for legitimate operator edge cases
**Alternatives considered:** Hardcoded immutable security context; permissive defaults with a "hardened" values preset; let operators opt in to restricted contexts
**Rationale:** The chart ships with restricted Pod Security Standards as the default values in `values.yaml` (`podSecurityContext` and `securityContext`). These defaults match what the distroless `:nonroot` image already guarantees at UID 65532. The fields are exposed in `values.yaml` and `values.schema.json`, allowing operators to override them via `--set` for legitimate edge cases: storage CSI drivers with non-standard UID constraints, sidecars requiring writable root filesystems, profilers needing elevated capabilities, etc. Organizations wanting to enforce the restricted profile cluster-wide should use ValidatingAdmissionPolicy or Kyverno at the cluster level, not chart-level immutability. This approach balances secure-by-default with operational flexibility.

The default Pod-level security context enforces:
```yaml
runAsNonRoot: true
runAsUser: 65532
runAsGroup: 65532
fsGroup: 65532
seccompProfile:
  type: RuntimeDefault
```

The default container-level security context enforces:
```yaml
allowPrivilegeEscalation: false
readOnlyRootFilesystem: true
capabilities:
  drop:
    - ALL
seccompProfile:
  type: RuntimeDefault
```

**Decision:** Two storage modes (ephemeral in-memory, external Postgres) with a template-render-time `{{ fail }}` guard tying multi-replica to Postgres
**Alternatives considered:** Single mode with external database always required (rejected — closes off the homelab "I just want to see it run" path); preserving SQLite as a single-replica durable mode (rejected per ADR 0003 D3 — dual SQL flavors with different forwarder implementations is too much surface for one feature); separate chart variants per mode (rejected — duplicates the values surface and operator decision overhead)
**Rationale:** ATC's primary early audience is homelab operators. Ephemeral mode (no `databaseUrl`, single replica) covers first-touch demos and CI; external Postgres covers production and any multi-replica deployment. Cross-field constraints (`replicaCount > 1` without a Postgres URL) are enforced at render time via `{{ fail }}` because JSON Schema cannot express conditional dependencies between fields. Per ADR 0003 D3.

**Decision:** Constant `RollingUpdate` strategy (no per-mode flips)
**Alternatives considered:** Conditional `Recreate` (rejected — was tied to `persistence.enabled`, now removed); operator-selectable `strategy` (rejected — adds surface without observed need); document-only approach with no enforcement
**Rationale:** Both supported storage modes are RWO-volume-free at the application layer. Ephemeral mode keeps state process-local and tolerates rolling pod handoff (state loss is implicit at restart). External Postgres mode keeps state in PG; replicas are symmetric and tolerate rolling handoff (per ADR 0002 D5). Neither mode can suffer the ReadWriteOnce-volume foot-gun that drove the previous conditional `Recreate`. A single constant `RollingUpdate` block (`maxSurge: 1, maxUnavailable: 0`) gives zero-downtime in both modes.

**Decision:** Push observability via OTLP to an operator-run collector; the chart documents the dependency, it does not bundle one
**Alternatives considered:** Bundled OTel collector sidecar; in-chart Prometheus scrape endpoint with optional ServiceMonitor (the prior shape — removed); chart-managed collector Deployment
**Rationale:** Operators already run their own observability stack — the author's homelab uses Grafana Alloy, work clusters typically run an opentelemetry-collector deployment or DaemonSet. Bundling a collector inside the chart would either duplicate infrastructure that already exists or force operators to disable a sidecar they cannot use. Pushing OTLP to a configurable endpoint keeps the chart focused on the application and lets operators decide whether the collector receives traces, metrics, or both, and what backends they fan out to (Tempo/Mimir/Loki, Jaeger, vendor APMs, etc.). The previous `metrics.*` block, the ServiceMonitor template, the dedicated metrics Service port, and the `/metrics` endpoint on `atc-server` are all gone — operators scrape the collector, not the application.

**Decision:** Dual chart publishing channels — OCI (`oci://ghcr.io/bojanrajkovic/charts/atc`) and a classic HTTP repo on GitHub Pages (`https://bojanrajkovic.github.io/atc/charts`)
**Alternatives considered:** OCI only; GitHub Pages only
**Rationale:** OCI is the canonical channel for OCI-native workflows and is the only channel that carries the Sigstore build-provenance attestation. GitHub Pages is the recommended channel for consumers without GHCR authentication — `helm repo add` works against any laptop or CI without registry credentials. Both channels are tag-triggered from the same workflow and gated so the Pages publish only runs after the OCI publish succeeds; chart versions stay in lockstep. See `docs/architecture/release-pipeline.md` for the workflow shape and the manual `gh-pages` Pages-source prerequisite.

**Decision:** Optional NetworkPolicy with permissive defaults; operators harden in production
**Alternatives considered:** Ship a hardened default (rejected — requires operators to know their ingress controller and monitoring namespaces ahead of install, breaks the first-touch demo path); omit the resource entirely and require operators to author their own (rejected — leaves the chart without an opinionated peer-allowlist surface and forces ad-hoc YAML in every deployment); pin `policyTypes` to a fixed list independent of rule contents (rejected — diverges from Kubernetes' `policyTypes`-mirrors-`spec` convention and surprises operators reading the rendered manifest)
**Rationale:** A NetworkPolicy resource is only authored when `networkPolicy.enabled=true`. The default ingress and egress rule lists permit traffic to the chart's HTTP port and DNS lookups to `kube-system`, with an open egress fallback for GitHub API and Postgres traffic — enough to install cleanly into any cluster without prior knowledge of the operator's network topology. The chart documents (here and in `values.yaml` comments) that operators are expected to restrict `from`/`to` peers in production. `policyTypes` mirrors which of `ingress` / `egress` is **present** as a key (not whether the list has rules): omitting a key entirely (e.g. `networkPolicy.ingress: null`) drops that direction from `policyTypes`, while setting it to an empty list (`ingress: []`) keeps it in `policyTypes` and renders the empty rule list verbatim — preserving Kubernetes' "deny-all this direction" semantics. Rule items are passed through verbatim via `toYaml`, matching how the chart already handles `ingress.hosts` and `gateway.rules` — operators who need cluster-specific peers (`namespaceSelector`, `ipBlock`, `podSelector`) author them in their values overlay without chart changes.

**Decision:** Dual routing support — optional Ingress (`networking.k8s.io/v1`) and optional HTTPRoute (`gateway.networking.k8s.io/v1`)
**Alternatives considered:** Ingress only; Gateway API only; neither (document port-forward)
**Rationale:** Ingress covers clusters with a classic ingress controller (nginx, traefik). HTTPRoute covers clusters running a Gateway API controller (Envoy Gateway, Cilium). Providing both optional templates at the same chart version avoids forking. The default is neither — port-forward instructions in NOTES.txt cover the zero-dependency case.

## Environment Variables

The chart wires Helm values to two distinct env-var surfaces: the application's `ATC_*` config (canonical list in `backend/crates/atc-server/src/config.rs`) and the spec-standard `OTEL_*` envs read by the OpenTelemetry SDK.

### `ATC_DATABASE_LISTENER_URL`

| Property | Value |
|----------|-------|
| Type | Optional string |
| Default | Falls back to `ATC_DATABASE_URL` when unset |

**When to set:** When the main pool (`ATC_DATABASE_URL`) connects through transaction-mode PgBouncer, which reassigns the underlying connection between transactions and silently drops `LISTEN` registrations. Point this at a direct Postgres DSN or a session-mode PgBouncer endpoint.

**Helm:** Settable via:
- `config.databaseListenerUrl` (plain value, sets env var directly)
- `existingSecret.databaseListenerUrlKey` (secret key reference; wins over `config.databaseListenerUrl` when both are set)

When neither is set, the listener falls back to `ATC_DATABASE_URL` and no `ATC_DATABASE_LISTENER_URL` env entry is injected into the pod.

### `ATC_OUTBOX_RETENTION`

| Property | Value |
|----------|-------|
| Type | humantime-parseable duration string (`7d`, `24h`, `90m`) |
| Default | `7d` |
| Minimum supported | `1h` — values below the floor fail process startup |

**When to set:** Override the default retention age for the PG-mode outbox sweep. Shorter values reclaim disk earlier; longer values give more headroom against replica outages (the sweep's safety floor never deletes rows below `MIN(broadcast_watermark)` across non-stale replicas).

**1 h hard floor.** `PgStore::start_inner` returns `PgStoreStartError::RetentionTooShort` and the process exits with a clear error message if `ATC_OUTBOX_RETENTION < 1h`. The floor exists because Postgres `inserted_at` defaults to `transaction_timestamp()` (transaction-start), not commit time — sub-floor retention is unsafe under MVCC for any non-instantaneous writer transaction. Operators needing shorter retention should file an issue and propose partition rotation instead.

**Rolling-deploy assumption.** Sub-hour retention is blocked at the 1 h floor, and the heartbeat task's `stale_threshold` is 90 s, so a Helm rolling update must complete within 1 h to avoid the edge case of an old-version replica having its uncommitted broadcasts retired by a new-version replica. Default `terminationGracePeriodSeconds` (30 s) and `maxSurge=1 maxUnavailable=0` ensure rolling updates complete in minutes, well under that budget. Default `7d` retention removes the constraint entirely.

**Helm:** Set via `config.outboxRetention` (default `"7d"`).

**Observability:** Three metrics watch retention health — see [`metrics.md`](metrics.md):
- `atc_pg_outbox_rows_deleted_total` (counter; per-replica share of cluster-wide deletion rate)
- `atc_pg_outbox_min_replica_watermark` (gauge; cluster-wide safety floor, NaN when no live replicas)
- `atc_pg_outbox_oldest_row_age_seconds` (gauge; oscillates near `outbox_retention` at steady state, NaN when empty)

Two per-tick root spans (`outbox.heartbeat.tick`, `outbox.sweep.tick`) carry replica id, watermark, and rows-deleted attributes for trace-based investigation.

See [ADR 0007](../architecture-decisions/0007-outbox-retention-policy.md) for the design rationale.

### OpenTelemetry (`OTEL_*`)

The deployment template injects five spec-standard env vars when `otel.enabled: true`:

| Env var | Helm key | Purpose |
|---------|----------|---------|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | `otel.endpoint` | OTLP/HTTP collector URL (e.g. `http://otel-collector.observability:4318`). Required when enabled. |
| `OTEL_SERVICE_NAME` | `otel.serviceName` | Resource attribute identifying the service. Defaults to `"atc"`. |
| `OTEL_RESOURCE_ATTRIBUTES` | `otel.resourceAttributes` | Comma-separated `key=value` pairs appended after auto-injected `k8s.*` identifiers (see below). E.g. `deployment.environment=production,service.namespace=ingest`. |
| `OTEL_TRACES_SAMPLER` | `otel.sampler` | Trace sampler. Defaults to `parentbased_always_on`. |
| `OTEL_TRACES_SAMPLER_ARG` | `otel.samplerArg` | Sampler argument (e.g. `"0.1"` for 10% root sampling with `parentbased_traceidratio`). REQUIRED non-empty when `otel.sampler` is a ratio sampler — render-time guard fails otherwise. |

The `otel.*` values block in `deploy/helm/atc/values.yaml` is the operator's contract for these envs — refer to that file for inline default values, comments, and any future additions. Transport is HTTP/protobuf only; there is no `protocol:` key, and `OTEL_EXPORTER_OTLP_PROTOCOL` is not injected. gRPC is out of scope and would require an opt-in build of `atc-server`.

**Pod-identity attributes auto-injected.** When `otel.enabled: true`, the deployment template also wires four downward-API env vars (`OTEL_K8S_POD_NAME`, `OTEL_K8S_POD_NAMESPACE`, `OTEL_K8S_POD_UID`, `OTEL_K8S_NODE_NAME`) and prepends them as OTel resource attributes to `OTEL_RESOURCE_ATTRIBUTES`:

```
k8s.pod.name=$(OTEL_K8S_POD_NAME),k8s.namespace.name=$(OTEL_K8S_POD_NAMESPACE),k8s.pod.uid=$(OTEL_K8S_POD_UID),k8s.node.name=$(OTEL_K8S_NODE_NAME),k8s.deployment.name=<release>-atc[,<otel.resourceAttributes>]
```

Operator-supplied `otel.resourceAttributes` are appended after this prefix, so any explicit `k8s.*` override in values WINS by virtue of coming later in the comma-separated list (the OTel SDK takes the last value for duplicate keys). The deployment name is computed from the chart's `atc.fullname` template at render time (downward API does not expose the owner workload's name). This matches the canonical OTel k8s semantic-conventions surface and removes per-environment values overrides for pod/node identity.

When `otel.enabled: false` (the default), none of the `OTEL_*` env vars are injected and the OTel SDK is never initialized inside the container — there is no provider, no exporter, no background-task overhead, and `metrics::*` macros resolve through the no-op recorder.

**Operator dependency.** Setting `otel.enabled: true` assumes an OTel collector is reachable at the configured endpoint. Operators bring their own collector (Grafana Alloy, opentelemetry-collector-contrib, vendor distributions, etc.) — the chart documents the dependency, it does not bundle one. The collector decides which downstream backends consume the OTLP stream.

## Authentication

**ATC ships no built-in authentication.** The SPA, `GET /v1/state`, and `GET /v1/ws` are all unauthenticated. The webhook endpoint at `POST /v1/webhooks/github` validates HMAC-SHA256 signatures when `ATC_GITHUB__WEBHOOK_SECRET` is configured; nothing else is gated. Anyone who can reach the HTTP port can read every run, job, and runner-pool record for every repository whose webhooks land on the deployment.

This is a deliberate scope decision. ATC is designed to live inside a trusted network surface and accept the surrounding deployment's identity model rather than ship its own auth subsystem (which would force OIDC / SAML / session-store choices that operators already make at the cluster edge). The three supported patterns are:

- **Private network** — VPC, homelab subnet, Tailscale tailnet, or any network where the access-control answer is "you have to already be inside."
- **Authenticating reverse proxy** — Pomerium (recommended), oauth2-proxy, or Authelia + nginx/Caddy. The SPA loads under the proxy's session cookie; the same cookie flows through to the WebSocket upgrade because they're same-origin. Cloudflare Access does not currently work for the live WebSocket route — see the operator runbook for the analysis and workarounds.
- **Ingress annotations** — wire any ingress-class-specific auth filter via the chart's `ingress.annotations` pass-through (nginx `auth_request`, Traefik middleware chains, etc.). Gateway API users attach auth through the API's native mechanisms instead (Envoy Gateway `SecurityPolicy`, HTTPRoute `filters` with `ExtensionRef`); the chart does not currently expose annotations on its `HTTPRoute` template.

**Per-proxy recipes**, the cross-cutting gotchas (`Origin` validation, cookie `SameSite`, idle-timeout starvation on the long-lived WS), and the path-split layout that lets `/v1/webhooks/github` bypass auth while the rest of the surface is gated all live in the operator runbook: [`docs/operator/authentication.md`](../operator/authentication.md).

**Not in scope (today):** first-class OIDC inside `atc-server`, per-repository or per-org access control, audit logging of frontend reads, native `Origin` validation on the WS endpoint. If any of these matter for your deployment, file an issue describing the operator surface you'd want.

## File-based configuration

`atc-server` loads its config through the [`figment`](https://docs.rs/figment) crate with a three-layer chain, lowest precedence to highest:

1. **Defaults** — struct-field defaults baked into the binary (`Config::default()` in `backend/crates/atc-server/src/config.rs`).
2. **YAML file** at `$ATC_CONFIG_FILE` (default `/etc/atc/config.yaml`). Missing file is benign — the layer contributes nothing.
3. **Environment** — variables prefixed `ATC_` (e.g. `ATC_HTTP_ADDR`, `ATC_DATABASE_URL`, `ATC_GITHUB__WEBHOOK_SECRET`).

Env carries **scalar overrides only**. Structured config — currently the `runner_pools` block — is file-only by design: figment's `Env::prefixed("ATC_").split("__")` provider does not natively decode arrays of objects from env vars, and adding a JSON-decoding shim is deferred (tracked in the [out-of-scope follow-up](#) for the runner-pool-capacity feature).

### Canonical mount path

The mount path defaults to `/etc/atc/config.yaml`. POSIX-conventional, plays cleanly with `readOnlyRootFilesystem: true`. Override with `ATC_CONFIG_FILE` if a different location is needed (useful for tests).

### `runnerPools` Helm values

The chart renders a `ConfigMap` and a read-only `volumeMount` only when `runnerPools` is non-empty:

```yaml
runnerPools:
  - labels: [self-hosted, linux, x64]
    capacity: 10              # bounded — declared ceiling, renders running/10 with a saturation bar
  - labels: [ubuntu-latest]
    capacity: null            # unbounded — no renderable ceiling, renders running with an ∞ affordance
```

Empty list (the default) ⇒ no `ConfigMap`, no volume, no behavior change. The in-memory dev mode and existing single-replica deployments stay byte-identical.

`values.schema.json` enforces the per-entry shape: `labels` is a non-empty array of unique strings, `capacity` is required and is either an integer ≥ 1 or `null`. `null` declares the pool unbounded (e.g. ARC `AutoscalingRunnerSet` without `maxRunners`, or GitHub-hosted runners whose per-account concurrency limits do not yield a per-label ceiling). Server-side validation additionally:
- Enforces explicit `capacity` key presence via a custom `Deserialize` impl — omitting the key is rejected with `"capacity is required (use \`capacity: null\` for an unbounded pool)"`. The JSON Schema also keeps `capacity` in `required`, so the failure surfaces at `helm install` / `helm upgrade` time AND at server startup.
- Canonicalizes labels (sort + dedup) implicitly during deserialization (the wire `LabelSet` is a `BTreeSet<String>` under the hood); a post-extract scan additionally rejects two entries that canonicalize to the same set — silently last-one-wins would be a deployment-time footgun.
- Rejects `capacity: 0` with `"capacity must be >= 1 (use null for unbounded pools)"`.

All failures are fatal at startup.

The parsed pool list is composed onto every `/v1/state` snapshot response as `runnerPoolCapacities` with shape `{ labels, capacity: number | null }` (ADR 0004 — frontend remains the single derivation point for pool stats; capacity arrives as inert config, never as a server-side metric). The frontend merges the wire field into `RunnerPoolStats.total` as a `RunnerPoolTotal` tagged sum: `Bounded(n)` for declared integers (lights up `CapacityBar.svelte` with the saturation color band), `Unbounded` for `null` (renders an ∞ affordance with an `aria-label="unbounded capacity"` to satisfy WCAG SC 1.4.1), `Undeclared` for pools observed via webhooks but absent from `runnerPools` (count only, no affordance).

### Hot-reload

Edits to `/etc/atc/config.yaml` are picked up without a pod roll. A `config_watcher` task (see `backend/crates/atc-server/src/config_watcher.rs`) watches the parent directory through `notify-debouncer-full` with a 500 ms debounce window — long enough to coalesce the burst of filesystem events that kubelet's ConfigMap atomic swap produces, short enough that an operator edit propagates within one second of kubelet completing the swap. End-to-end propagation time (ConfigMap edit → open browser sees new capacities) is ≤ 90 s: kubelet sync (default ~60 s) plus the debounce.

**Directory mount, not `subPath`.** The chart mounts the ConfigMap at `mountPath: /etc/atc` (directory) — Kubernetes explicitly documents that `subPath` ConfigMap mounts do NOT receive updates, so hot-reload requires the directory mount. kubelet exposes the projected directory via an atomic `..data` symlink swap; the watcher's parent-dir watch sees the rename and triggers a reload.

**Reload-only fields.** Only the `runner_pools` block hot-reloads. The narrow reload schema (`config::reload_runner_pools`) deliberately ignores scalar fields (`http_addr`, `database_url`, log settings, retention) so an operator editing a scalar in YAML doesn't appear to have the edit accepted-then-discarded — the watcher simply isn't looking at that field. A separate diagnostic compares the live file's scalars to a startup snapshot; each changed scalar field produces a `tracing::warn!` instructing the operator to roll the pod.

**Graceful failure.** When a reload fails (zero capacity, duplicate pool, malformed YAML, deleted file, read error) the previous valid capacities stay in place. The watcher logs a structured error, increments `atc_config_reload_total{result="failure",reason=<category>}`, and broadcasts a `ConfigReloadError` WS frame; the frontend logs the failure to the console (UI surfacing tracked in issue #203).

**Missing-file divergence.** Startup tolerates a missing config file (figment's `Yaml::file` is auto-optional, yielding `runner_pools: []`). On reload, a deleted file is treated as `ReloadError::Read` — an operator who deletes the file mid-deploy almost certainly didn't intend to clear all pool capacities, so the old caps are kept and the error is surfaced.

**Wire framing.** The WS endpoint frames every event in an outer `kind` discriminator: `Committed` (the existing seq-keyed `CommittedEvent`), `ConfigUpdate { runnerPoolCapacities }` (full capacity list after a successful reload, not a delta), and `ConfigReloadError { reason }`. The dispatcher's outer-kind switch routes Committed frames through the RAF-batched store path and applies ConfigUpdate / ConfigReloadError immediately. Lagged on either channel closes the WS connection symmetrically — the client reconnects and re-fetches `/v1/state` to re-establish both the seq cursor and the current capacities.

**Shutdown.** The watcher task is joined explicitly by `run_shutdown_orchestration` under `SHUTDOWN_TIMEOUT_CONFIG_WATCHER` (1 s) before OTel pipeline tear-down, matching the "no live emitter when shutdown fires" invariant.

**Bare-metal dev boxes.** If the parent directory of `$ATC_CONFIG_FILE` doesn't exist (typical on a dev box without `/etc/atc/`), the watcher is skipped with a warn log and the process boots cleanly without hot-reload.

## Multi-replica

`replicaCount > 1` requires a PostgreSQL connection string via either `config.databaseUrl` or `existingSecret.name`+`existingSecret.databaseUrlKey`. The chart enforces this at template-render time with a `{{ fail }}` guard at the top of `templates/deployment.yaml`. The same template also rejects any non-PostgreSQL scheme on the inline `config.databaseUrl` path (`postgres://` and `postgresql://` are the only accepted prefixes); the `existingSecret` path is opaque at render time and falls through to a startup-time scheme check in the binary (`ensure_pg_scheme()` in `backend/crates/atc-server/src/main.rs`) that exits with a remediation-naming log line before any sqlx connect call. The chart's ephemeral in-memory mode is single-replica only.

The runtime invariants that make symmetric multi-replica safe (see [`state-externalization-research/`](state-externalization-research/README.md)):

- **Per-replica `broadcast_watermark`.** Each replica owns an `Arc<AtomicI64>` cursor advanced by the drain task only after a successful drain pass. `/v1/state` snapshot reads do an `Acquire` load before opening the snapshot transaction; the snapshot's `lastSeq` is bounded by what that replica has actually broadcast.
- **REPEATABLE READ snapshots.** `/v1/state` opens a REPEATABLE READ transaction that reads `runs`, `jobs`, and `MAX(outbox.seq)` from one MVCC snapshot. Without this isolation level, a concurrent webhook commit between the runs SELECT and the seq SELECT could advance `lastSeq` past content the snapshot hasn't materialized — the frontend's `seq > lastSeq` filter would then permanently drop a real event.
- **Ring-buffer dedup (single-delivery contract).** The drain task carries a 2048-seq ring buffer (~16 KB per replica) and skips a NOTIFY-driven row if its seq has already been broadcast on this replica. This preserves ADR 0003's no-frontend-dedup stance under gap-healing rescans (a re-fetched row never reaches the WS broadcast a second time).
- **Drain heartbeat readiness.** `/readyz` 503s when the drain heartbeat is older than 30s, and short-circuits to 503 `{"status":"shutting_down"}` once `state.shutdown` is cancelled (see § Graceful shutdown below). Routes a misbehaving or terminating replica out of Service endpoints fast enough that a client reconnect lands on a healthy peer.

**Sticky sessions are NOT required** and are discouraged outside specific cost-tuning scenarios. Reconnect-then-snapshot via `/v1/state`+`lastSeq` is the design (ADR 0002 D5). A client that always lands on the same replica via sticky cookies will never exercise the reconnect-across-replicas code path, masking gap-healing regressions in development. Operators with specific needs (e.g., reducing reconnect storms during rolling updates) can add sticky-cookie annotations themselves at the Ingress / HTTPRoute level — the chart does not do this by default.

**Pod anti-affinity** ships on by default. The chart injects an `affinity.podAntiAffinity` rule keyed off the standard selector labels (`app.kubernetes.io/name` + `app.kubernetes.io/instance`) at `topologyKey: kubernetes.io/hostname`, controlled by `podAntiAffinity.type`:

| `podAntiAffinity.type` | Behavior |
|------------------------|----------|
| `soft` (default) | `preferredDuringSchedulingIgnoredDuringExecution` with weight `100`. The scheduler tries to spread replicas across nodes but does not refuse to schedule when only one node is available — safe for single-node homelab clusters and kind/k3d. |
| `hard` | `requiredDuringSchedulingIgnoredDuringExecution`. Replicas refuse to schedule onto a node already running another ATC replica from the same release. Only safe when the cluster has at least `replicaCount` schedulable nodes. |
| `off` | Chart omits the `affinity` block entirely. |

Setting `affinity:` to a non-empty value (full operator override) takes precedence over `podAntiAffinity.type` — the chart renders the supplied value verbatim and skips the convenience injection. Use this for compound affinity needs that mix `nodeAffinity`, `podAffinity`, custom weights, or non-hostname topology keys.

## Autoscaling

The chart renders an optional `HorizontalPodAutoscaler` (`autoscaling/v2`) gated on `autoscaling.enabled`. When enabled, the chart drops `spec.replicas` from the Deployment so the HPA owns the replica count — the autoscaler treats `replicas` as an external mutation otherwise, and operators see warning events on every reconcile. Operators set `autoscaling.minReplicas` for floor-count semantics.

The HPA emits a CPU `Resource` metric with `target.type: Utilization` driven by `autoscaling.targetCPUUtilizationPercentage` (default 80) and, when `autoscaling.targetMemoryUtilizationPercentage` is non-null, a memory `Resource` metric alongside it. Either target may be set to `null` to omit the corresponding metric (e.g., for memory-only autoscaling); when both are null, the HPA renders without a `metrics` block (uncommon — typically paired with custom `metrics.external` set elsewhere).

**Resource-request precondition.** Utilization-based metrics divide observed usage by the Pod's `resources.requests.<resource>`. When the request is unset, metrics-server reports `FailedGetResourceMetric` / `missing request for cpu` and the HPA never makes a scaling decision. The chart enforces this at template-render time: setting `autoscaling.targetCPUUtilizationPercentage` (default 80) without `resources.requests.cpu` fails the render with a remediation message, and the same guard pairs `autoscaling.targetMemoryUtilizationPercentage` with `resources.requests.memory`. The guard fires before any HPA is created, so operators see the misconfig at `helm install` time rather than after a load test.

**Multi-replica precondition extends to autoscaling.** The same `{{ fail }}` guard that rejects `replicaCount > 1` without a Postgres URL also fires when `autoscaling.enabled` is true with `autoscaling.maxReplicas > 1`. The guard tests `maxReplicas` (not `minReplicas`) because the autoscaler is allowed to scale up to that ceiling — any unguarded ceiling above 1 admits divergent in-memory state. When `autoscaling.enabled` is true, the guard considers ONLY `autoscaling.maxReplicas` and ignores `replicaCount`, because the HPA owns `spec.replicas` (replicaCount is dead config in that mode); a high `replicaCount` paired with `autoscaling.maxReplicas: 1` is therefore valid for in-memory single-replica installs. When `autoscaling.enabled` is false, the guard falls back to `replicaCount > 1`.

**Knobs.**

| Values key | Default | Notes |
|-----------|---------|-------|
| `autoscaling.enabled` | `false` | When false, the Deployment uses `replicaCount` directly and no HPA is rendered. |
| `autoscaling.minReplicas` | `1` | Floor; respected during low-traffic windows. |
| `autoscaling.maxReplicas` | `5` | Ceiling; values > 1 require a Postgres URL via the multi-replica fail-guard. |
| `autoscaling.targetCPUUtilizationPercentage` | `80` | CPU `Resource` metric target. When set, requires `resources.requests.cpu`. Set to `null` to omit the CPU metric. |
| `autoscaling.targetMemoryUtilizationPercentage` | `null` | When set, appends a memory `Resource` metric alongside CPU; requires `resources.requests.memory`. |

## PodDisruptionBudget

The chart renders an optional `policy/v1` PodDisruptionBudget at `templates/pdb.yaml`, gated on `podDisruptionBudget.enabled` (default `false`). Single-replica deployments don't benefit from a PDB, and operators running multi-replica should opt in explicitly so the chart never silently blocks node drains for ephemeral-mode users.

**Mutual-exclusion guard.** Kubernetes' PDB schema rejects setting both `minAvailable` and `maxUnavailable` on the same object. The chart enforces this at template-render time via a `{{ fail }}` guard at the top of `templates/pdb.yaml`: set one, leave the other `null`. Both fields accept either an integer (pod count) or a percentage string like `"50%"`.

**Selector scope.** The PDB selector uses `atc.selectorLabels` (`app.kubernetes.io/name` + `app.kubernetes.io/instance`), matching the Deployment's pod template. The chart-version-bearing labels emitted by `atc.labels` are deliberately excluded so the selector remains immutable across chart upgrades.

**Knobs.**

| Values key | Default | Notes |
|-----------|---------|-------|
| `podDisruptionBudget.enabled` | `false` | Opt-in. Recommended for any multi-replica deployment. |
| `podDisruptionBudget.minAvailable` | `1` | Integer or percentage string. Mutually exclusive with `maxUnavailable`. |
| `podDisruptionBudget.maxUnavailable` | `null` | Integer or percentage string. Mutually exclusive with `minAvailable`. |

**Operator guidance.** For production multi-replica deployments, the conservative default (`minAvailable: 1`) keeps at least one replica serving during voluntary disruptions (node drains, autoscaler scale-down). Operators running three or more replicas may prefer `maxUnavailable: 1` to allow concurrent drains across nodes without stalling cluster maintenance.

## Graceful shutdown

The chart pairs Kubernetes' pod-termination lifecycle with the in-process cooperative shutdown orchestration in `atc-server` (~13 s aggregate budget; see `docs/architecture/backend-server.md` § Operator shutdown contract).

**preStop hold.** A `lifecycle.preStop.sleep` action runs before the kubelet sends SIGTERM. It absorbs the propagation delay between the EndpointSlice flip (`ready=false, serving=true, terminating=true` per `ProxyTerminatingEndpoints`) and kube-proxy / cloud-LB rule sync. The chart uses Kubernetes' native `Sleep` action (KEP-3960, beta default-on in 1.30, GA in 1.33) — chart `kubeVersion` is `>=1.32.0-0`. The native action is required because the runtime image is `gcr.io/distroless/cc-debian13:nonroot` and ships no `sleep` binary or shell; an `exec` preStop with `["/bin/sleep", "5"]` would `ENOENT` at runtime.

**terminationGracePeriodSeconds.** The pod-spec field is rendered from `shutdown.terminationGracePeriodSeconds` (default `30`). Sized so that `preStopSleepSeconds + 13 s` (the aggregate shutdown budget) fits inside the grace period with headroom. Lowering this without also reducing the cooperative-shutdown budget risks SIGKILL during the drain window.

**Readiness probe shutdown awareness.** `/readyz` short-circuits to 503 `{"status":"shutting_down"}` once the pod's `shutdown` token is cancelled — independent of the EndpointSlice flip. This is what cloud-LB controllers (AWS LBC, GCE NEG, Azure AGIC) and service meshes that watch readiness probes directly observe to start their own deregistration clocks. `/healthz` continues returning 200 unconditionally; the kubelet's liveness probe must not restart the pod mid-drain.

**Timeline.**

| Step | Source | Default |
|------|--------|---------|
| 1. EndpointSlice flips to `terminating` | kubelet / endpoint-slice controller | immediate on `deletionTimestamp` |
| 2. kubelet runs `preStop sleep` | `shutdown.preStopSleepSeconds` | 5 s |
| 3. kubelet sends SIGTERM | — | — |
| 4. cooperative shutdown sequence | `shutdown.rs` | up to 13 s |
| 5. SIGKILL if not exited | `shutdown.terminationGracePeriodSeconds` | 30 s budget |

**Knobs.**

| Values key | Default | Notes |
|-----------|---------|-------|
| `shutdown.preStopSleepSeconds` | `5` | Set to `0` to disable the hook (the chart omits the `lifecycle` block; required because Kubernetes rejects `Sleep.Seconds <= 0`). |
| `shutdown.terminationGracePeriodSeconds` | `30` | Should be `>= preStopSleepSeconds + 13`. Soft constraint, not validated by the chart — operators tuning either knob downward should keep this relation in mind. |

## NetworkPolicy

The chart renders an optional `networking.k8s.io/v1` NetworkPolicy when `networkPolicy.enabled=true` (default `false`). The resource scopes to the ATC pod via `atc.selectorLabels` and exposes `ingress` / `egress` as plain rule lists that are passed through verbatim via `toYaml`. `policyTypes` mirrors which keys are **present** under `networkPolicy`, distinguishing two operator intents that Kubernetes treats differently:

| Intent | Values shape | Rendered manifest |
|---|---|---|
| No constraint on a direction | key omitted (`networkPolicy.ingress: null`) | direction absent from `policyTypes`; no `ingress:` field |
| Default-deny a direction | key present, empty (`networkPolicy.ingress: []`) | direction in `policyTypes`; `ingress: []` rendered literally |
| Allow listed peers | key present with rules (`networkPolicy.ingress: [{...}]`) | direction in `policyTypes`; rules rendered |

Setting `ingress: []` and `egress: []` together yields the canonical isolation policy (deny all in both directions).

**Defaults are permissive — harden in production.** The chart-shipped defaults permit inbound traffic to the chart's HTTP port from any namespace, DNS lookups to `kube-system`, and outbound TCP/443 for the GitHub API. The PostgreSQL peer is intentionally not in the default egress because the chart does not know whether the database lives in a sibling namespace, a managed RDS-class endpoint outside the cluster, or a `podSelector`-addressable pod — operators add a rule scoped to their topology. Operators are also expected to restrict the inbound `from` list to ingress-controller and monitoring namespaces in production. Example operator overlay:

```yaml
networkPolicy:
  enabled: true
  ingress:
    - from:
        - namespaceSelector:
            matchLabels:
              kubernetes.io/metadata.name: ingress-nginx
        - namespaceSelector:
            matchLabels:
              kubernetes.io/metadata.name: monitoring
      ports:
        - port: http
          protocol: TCP
  egress:
    - to:
        - namespaceSelector:
            matchLabels:
              kubernetes.io/metadata.name: kube-system
      ports:
        - port: 53
          protocol: UDP
        - port: 53
          protocol: TCP
    - to:
        - namespaceSelector:
            matchLabels:
              kubernetes.io/metadata.name: postgres
      ports:
        - port: 5432
          protocol: TCP
```

**`kubeVersion` requirement.** NetworkPolicy `networking.k8s.io/v1` is GA on every supported cluster (the chart's `kubeVersion: ">=1.32.0-0"` already covers it). Authors should still verify the cluster runs a CNI that enforces NetworkPolicy (Calico, Cilium, kube-router, etc.) before flipping `enabled: true` — many CNIs install successfully without policy enforcement and the resource becomes a no-op.

## Grafana dashboard

The chart ships an opt-in Grafana dashboard at `deploy/helm/atc/dashboards/atc-overview.json` covering HTTP, webhook ingestion, PG outbox+drain pipeline, watermarks, retention, startup/shutdown lifecycle, process resource usage, and config reloads. Default off — provisioning a dashboard is an operator decision. Two discovery paths are supported from one chart-shipped JSON; enable whichever (or both) matches your stack.

### Discovery paths

| Path | Operator-side prerequisite | values toggle |
|---|---|---|
| kube-prometheus-stack Grafana sidecar (`kiwigrid/k8s-sidecar`) | Grafana deployed with the dashboard sidecar enabled; sidecar discovers ConfigMaps labeled `grafana_dashboard: "1"` in the configured namespace(s). | `grafanaDashboard.enabled: true` |
| grafana-operator v5 `GrafanaDashboard` CR | grafana-operator (`grafana.integreatly.org/v1beta1`) CRDs installed in the cluster; a `Grafana` instance matched by `instanceSelector`. | `grafanaDashboard.enabled: true` AND `grafanaDashboard.grafanaOperator.enabled: true` |

The CR's `spec.configMapRef` references the same ConfigMap the sidecar discovers — one JSON source, two delivery paths. Enable both safely when transitioning from one mechanism to the other or when running both in parallel.

### Values reference

| Key | Default | Purpose |
|---|---|---|
| `grafanaDashboard.enabled` | `false` | Master toggle. When `true`, renders the ConfigMap. |
| `grafanaDashboard.namespace` | `""` | Override namespace for the ConfigMap. Empty renders in the release namespace. Set when the Grafana sidecar discovers from a specific namespace. |
| `grafanaDashboard.labels` | `{grafana_dashboard: "1"}` | Discovery labels. `grafana_dashboard: "1"` is the kiwigrid/k8s-sidecar default. Override the value or key if your stack uses a different convention. |
| `grafanaDashboard.annotations` | `{}` | ConfigMap annotations. Common values: `grafana_folder: "ATC"` (folder placement), `k8s-sidecar-target-directory: "/..."` (on-disk path override). |
| `grafanaDashboard.grafanaOperator.enabled` | `false` | Additionally render the GrafanaDashboard CR. Requires grafana-operator CRDs installed. |
| `grafanaDashboard.grafanaOperator.instanceSelector` | `{matchLabels: {dashboards: "grafana"}}` | LabelSelector for the target `Grafana` CR(s). Pass-through with `matchLabels` / `matchExpressions` shape validation. |
| `grafanaDashboard.grafanaOperator.folderRef` | `""` | Name of a `GrafanaFolder` CR in the same namespace. Empty omits the field. |
| `grafanaDashboard.grafanaOperator.resyncPeriod` | `""` | Go duration string (e.g. `"5m"`). Empty omits the field; the operator uses its own default. |

### Datasource portability

Every panel's datasource reference is `{ "type": "prometheus", "uid": "${datasource}" }`. The dashboard declares a `datasource` template variable of type `datasource` with `query: prometheus`, so Grafana resolves the variable against whichever Prometheus datasource(s) the operator has configured. Operators with one Prometheus get automatic resolution; operators with multiple get a top-of-dashboard picker. No `${DS_PROMETHEUS}` / `__inputs` block is shipped — the variable approach handles both file-provisioned discovery (sidecar / operator) AND standalone Grafana-UI import without requiring chart-side string substitution.

### Histogram-aggregation assumption

Panel queries use the **native histogram** form: `histogram_quantile(0.99, sum by (label) (rate(name[5m])))`. The OTel SDK uses `Base2ExponentialHistogram` aggregation, and the OTLP→Prometheus translator surfaces those as native histograms when the storage supports them (Prometheus 2.40+, Mimir, the grafana/otel-lgtm bundle). Operators running collectors that emit only classic histograms (older stacks, transitional configurations) must translate panel queries to the classic form: `histogram_quantile(0.99, sum by (le, label) (rate(name_bucket[5m])))`. See `docs/architecture/metrics.md` § Histogram aggregation.

### Standalone import

The dashboard is also importable via the Grafana UI without the chart — fetch `deploy/helm/atc/dashboards/atc-overview.json` from the repo and use **Dashboards → New → Import**. The `${datasource}` variable resolves the same way it does under chart-bundled discovery.

## Testing Conventions

helm-unittest suites live in `deploy/helm/atc/tests/unit/*.yaml` and are run via `helm unittest deploy/helm/atc`. Scope them to invariants and conditionals, not template tautologies.

**Assert:**

- **Security invariants** — PSS restricted fields (`runAsNonRoot`, `runAsUser`, `readOnlyRootFilesystem`, `capabilities.drop`, `seccompProfile.type`) in the default render, so a future PR that removes one ships a visible test failure rather than an insecure chart.
- **Conditional-branch flips** — both sides of any business-logic branch. For example, `strategy.type` toggling between `RollingUpdate` and `Recreate` based on `persistence.enabled`.
- **`{{ fail }}` guards** — every guard gets a dedicated test that sets the conflicting values and asserts the render fails with the expected message. An untested guard can be silently broken by a refactor.
- **Cross-template invariants** — e.g. a PVC's `metadata.name` MUST equal the Deployment's volume `claimName`. These catch the class of bug that motivated this convention.

**Skip:**

- **Tautological field assertions** on static content (Ingress className, TLS hosts, ServiceMonitor intervals). They duplicate the template. Kubeconform validates schema; diff review catches semantic regressions.
- **Rendered-kinds-under-defaults** assertions. The individual `if` gates are themselves the test; Kubeconform would still validate any mistakenly-rendered resources.
- **Content assertions on optional templates** (Ingress, PVC, ServiceMonitor, HTTPRoute) beyond what a conditional-branch or invariant check requires.

## Multi-replica smoke test

Operationally validate a two-replica deploy against a real cluster (kind/k3d/homelab).

> **Why this is a runbook, not CI.** Issue #12 tracks adding kind-based chart-testing to CI. Execute this once before declaring issue #7 closed; PRs do not re-run it.

Prerequisites: a Kubernetes cluster (`kind create cluster`, `k3d cluster create`, OrbStack, or any homelab cluster), `kubectl`, `helm`, `node` (for the WebSocket tap), `curl`, and `jq`. `helm`, `kubeconform`, `node`, and `jq` are all provisioned by `mise install` — invocations below assume `mise activate` is wired into your shell, otherwise prefix them with `mise exec --`. A reachable PostgreSQL — provision in-cluster via the bitnami PG chart, or point `databaseUrl` at an existing instance.

> **WebSocket tap.** The runbook uses `scripts/ws-tap.js` (a ~30-line Node WebSocket client) instead of `wscat` for capturing event streams. `wscat` is readline/TTY-bound and silently produces no output when redirected to a file, so it can't be used in a scripted single-delivery assertion. `wscat` remains the right tool for interactive WebSocket debugging — install it ad-hoc with `mise use npm:wscat` if you need it for that.

```bash
set -euo pipefail

# 1. Provision PostgreSQL (skip if you already have one).
helm install pg oci://registry-1.docker.io/bitnamicharts/postgresql \
  --set auth.username=atc --set auth.password=atc --set auth.database=atc

# 2. Install ATC with two replicas pointing at the PG.
helm install atc deploy/helm/atc \
  --set replicaCount=2 \
  --set config.databaseUrl=postgres://atc:atc@pg-postgresql:5432/atc

# Wait for both pods Ready.
kubectl rollout status deploy/atc

# 3. Port-forward each pod to a distinct local port.
PODS=( $(kubectl get pod -l app.kubernetes.io/name=atc -o name) )
kubectl port-forward "${PODS[0]}" 8081:8080 >/tmp/pf-a.log 2>&1 &
PF_A=$!
kubectl port-forward "${PODS[1]}" 8082:8080 >/tmp/pf-b.log 2>&1 &
PF_B=$!

# Open a WebSocket tap to each pod's /v1/ws (capture each session to a logfile).
# scripts/ws-tap.js is a small pipe-friendly Node WebSocket client; see the note
# in the prerequisites section above for why this isn't wscat.
node scripts/ws-tap.js ws://localhost:8081/v1/ws > /tmp/ws-a.log 2> /tmp/ws-a.err &
WS_A=$!
node scripts/ws-tap.js ws://localhost:8082/v1/ws > /tmp/ws-b.log 2> /tmp/ws-b.err &
WS_B=$!

# 4. Start a /readyz watchdog covering steps 4–6 (fails the run if either replica
# returns non-200 at any sample point during the test window).
( while kill -0 $$ 2>/dev/null; do
    for port in 8081 8082; do
      code=$(curl -s -o /dev/null -w '%{http_code}' "http://localhost:${port}/readyz")
      if [[ "$code" != "200" ]]; then
        echo "FAIL: /readyz on port ${port} returned ${code}" >&2
        kill $$
        exit 1
      fi
    done
    sleep 0.5
  done ) &
READYZ_WATCH=$!

# 5. POST a webhook (replay a captured GitHub Actions webhook with valid HMAC).
# The route is /v1/webhooks/github (see backend/crates/atc-server/src/routes.rs).
curl -fsS -X POST http://localhost:8081/v1/webhooks/github \
  -H "X-GitHub-Event: workflow_run" \
  -H "X-Hub-Signature-256: sha256=<HMAC>" \
  --data @/path/to/captured-webhook.json

# 6. Within 5 seconds, both pod-local /v1/state endpoints should converge on the
# same lastSeq. Fail the run if convergence does not happen in time.
deadline=$(( $(date +%s) + 5 ))
converged=0
while (( $(date +%s) < deadline )); do
  A=$(curl -fsS http://localhost:8081/v1/state | jq -r .lastSeq)
  B=$(curl -fsS http://localhost:8082/v1/state | jq -r .lastSeq)
  if [[ "$A" == "$B" && "$A" != "null" && "$A" != "0" ]]; then
    echo "converged at lastSeq=$A"
    converged=1
    break
  fi
  sleep 0.1
done
if (( converged != 1 )); then
  echo "FAIL: /v1/state lastSeq did not converge within 5s (A=$A, B=$B)" >&2
  exit 1
fi

# 7. Each WS-tap logfile must show exactly one CommittedEvent for the webhook
# (single-delivery via ring-buffer dedup). Allow up to 2s for live delivery.
sleep 2
COUNT_A=$(grep -c '"seq":' /tmp/ws-a.log 2>/dev/null || true)
COUNT_B=$(grep -c '"seq":' /tmp/ws-b.log 2>/dev/null || true)
COUNT_A=${COUNT_A:-0}
COUNT_B=${COUNT_B:-0}
if (( COUNT_A != 1 )) || (( COUNT_B != 1 )); then
  echo "FAIL: expected exactly one CommittedEvent per replica (A=$COUNT_A, B=$COUNT_B)" >&2
  exit 1
fi
echo "single-delivery verified: A=1 B=1"

# 8. Tear down the watchdog and port-forwards. Reaching here means /readyz stayed
# 200 throughout (the watchdog kills $$ on any sample failure above).
kill "$READYZ_WATCH" "$WS_A" "$WS_B" "$PF_A" "$PF_B" 2>/dev/null || true
wait 2>/dev/null || true
echo "PASS: multi-replica smoke test"
```

Pass criteria:
- Both `/v1/state` endpoints converge on the same `lastSeq` within 5 seconds of the webhook POST.
- Each WS-tap logfile shows exactly one `CommittedEvent` for the webhook.
- Both `/readyz` endpoints return 200 throughout the test.

`kubectl logs -l app.kubernetes.io/name=atc -f --prefix` tags each line with the pod name — sufficient for "which replica did what" attribution during inspection. Per-process replica identification at the metrics layer is added at the collector by the standard target attributes (`pod`, `instance`) — the `atc_pg_*` metrics ship unlabeled per-process and dashboards aggregate `by (pod)`. See `docs/architecture/metrics.md` § Operational metrics for the per-metric scoping rules.

### Re-running the smoke test against the same cluster

Replays of the same fixture against a non-empty PG return `{"status":"rejected"}` because the predicated `UPDATE … WHERE status IN (predecessors)` clause yields 0 rows on idempotent or invalid transitions (per [ADR 0002 D2](../architecture-decisions/0002-state-externalization-postgres-outbox.md)). To re-run cleanly, both PG state AND the per-replica `broadcast_watermark` must be reset:

```bash
# 1. Truncate the durable state.
kubectl -n atc-smoke exec deploy/postgres -- psql -U atc -d atc -c \
  "TRUNCATE outbox, jobs, runs RESTART IDENTITY CASCADE;"

# 2. Restart ATC pods so each replica re-reads COALESCE(MAX(seq),0)=0 into its
# in-process broadcast_watermark cursor. Without this step the drain task on
# each replica still has a non-zero watermark from the prior run, so the
# replayed seq=1 row is below-watermark and never broadcast.
kubectl -n atc-smoke rollout restart deploy/atc
kubectl -n atc-smoke rollout status deploy/atc
```

## Boundaries

**Owns:** Kubernetes resource templates (Deployment, Service, ServiceAccount, and optional Ingress/HTTPRoute/HPA/PodDisruptionBudget/NetworkPolicy), values schema validation, post-install operator guidance (NOTES.txt), chart packaging, and chart publishing on the OCI and GitHub Pages channels
**Does not own:** Container image build (Dockerfile, release.yml), backend configuration (ATC_* env vars are the interface), Kubernetes cluster provisioning, Ingress controller or Gateway controller installation, PostgreSQL provisioning
**Prohibitions:** Do not embed secrets in chart templates — operators must use `existingSecret` or provide values at install time.

## Files

- `deploy/helm/atc/Chart.yaml` — Chart identity (name, version, appVersion, kubeVersion, maintainers)
- `deploy/helm/atc/values.yaml` — Full values surface with inline documentation of the two storage modes
- `deploy/helm/atc/values.schema.json` — JSON Schema draft 2020-12 for all values fields; `additionalProperties: false` rejects unknown keys at install/upgrade/lint time
- `deploy/helm/atc/LICENSE` — Apache-2.0 license (copy of repo root LICENSE)
- `deploy/helm/atc/.helmignore` — Excludes CI test fixtures and helm-docs source template from the chart tarball
- `deploy/helm/atc/templates/_helpers.tpl` — Named template helpers: `atc.name`, `atc.fullname`, `atc.chart`, `atc.labels`, `atc.selectorLabels`, `atc.serviceAccountName`
- `deploy/helm/atc/templates/deployment.yaml` — Mandatory workload with constant `RollingUpdate` strategy, restricted PSS security contexts, multi-replica `{{ fail }}` guard, env var wiring, and a `tmp` `emptyDir` volume mount
- `deploy/helm/atc/templates/service.yaml` — ClusterIP Service exposing the `http` port
- `deploy/helm/atc/templates/serviceaccount.yaml` — ServiceAccount gated on `serviceAccount.create`; `automountServiceAccountToken: false`
- `deploy/helm/atc/templates/NOTES.txt` — Post-install guidance with conditional ingress/gateway/port-forward branches and plain-credentials warning
- `deploy/helm/atc/templates/ingress.yaml` — Optional Ingress (`networking.k8s.io/v1`), gated on `ingress.enabled`; supports TLS, hosts, and custom annotations
- `deploy/helm/atc/templates/httproute.yaml` — Optional HTTPRoute (`gateway.networking.k8s.io/v1`), gated on `gateway.enabled`; validates non-empty `parentRefs` via `{{ fail }}` guard
- `deploy/helm/atc/templates/networkpolicy.yaml` — Optional NetworkPolicy (`networking.k8s.io/v1`), gated on `networkPolicy.enabled`; selectorLabels scope, `policyTypes` mirrors which of `ingress` / `egress` keys are present (so `ingress: []` renders as default-deny ingress; key omitted means no constraint on that direction), rule items pass through verbatim
- `deploy/helm/atc/templates/hpa.yaml` — Optional HorizontalPodAutoscaler (`autoscaling/v2`), gated on `autoscaling.enabled`; targets the chart Deployment with CPU `Resource` metric (always) and an optional memory `Resource` metric
- `deploy/helm/atc/templates/pdb.yaml` — Optional PodDisruptionBudget (`policy/v1`), gated on `podDisruptionBudget.enabled`; `minAvailable` and `maxUnavailable` are mutually exclusive (chart fails template rendering when both are set)
- `deploy/helm/atc/templates/tests/test-connection.yaml` — Helm test hook Pod with restricted Pod Security Standards; validates Service connectivity; excluded from charts via `helm.sh/hook: test` annotation
- `deploy/helm/atc/tests/values-*.yaml` — CI values sweep (defaults, ingress, gateway, multi-replica, otel, existing-secret-listener, pdb, networkpolicy, autoscaling) consumed by `scripts/helm-kubeconform.sh` via `helm template | kubeconform`; excluded from chart tarball by `.helmignore /tests/` anchor
- `deploy/helm/atc/ci/test-values.yaml` — `ct install` fixture consumed by the `helm-install` CI job (image override + `pullPolicy: Never` for the kind-loaded local image). See `docs/architecture/ci-pipeline.md` for the job definition.

## Storage modes

The chart supports two storage modes — ephemeral in-memory and external Postgres — per ADR 0003 D3. SQLite was considered and rejected:

- **SQLite not supported.** SQLite has no `LISTEN/NOTIFY` equivalent. Supporting it as a single-replica durable mode would require dual SQL flavors with different forwarder implementations (Postgres push, SQLite poll). The maintenance and test-matrix cost of dual SQL backends outweighs the value of "single-binary + PVC durable mode" as a deployment shape.
- **No `persistence.*` chart machinery.** The chart has no PVC template, `persistence:` values block, or persistence-conditional volume mounts. An audit found no application-code consumer of Kubernetes PVCs (only in-memory state, sessionStorage/localStorage in the frontend, and the PostgreSQL layer). With zero current or planned consumers, a templated PVC would be dead code.
- **Constant `RollingUpdate` strategy.** Both supported modes are RWO-volume-free, so a constant `RollingUpdate` (`maxSurge: 1, maxUnavailable: 0`) gives zero-downtime in both.
- **Multi-replica precondition guard.** A template-render-time `{{ fail }}` guard rejects `replicaCount > 1` without a Postgres URL (via either `config.databaseUrl` or `existingSecret`).

Operators whose values files contain a `persistence:` key will see schema validation reject the unknown property (`additionalProperties: false`). Mitigation: remove the `persistence:` block from operator values files. There is no programmatic migration tool — this is a deliberate breaking change in a 0.x chart.

If a future use case requires PVC-backed storage (e.g., a sidecar buffering audit logs to disk), the surface should be re-introduced tightly scoped to that consumer rather than as a general-purpose toggle.
