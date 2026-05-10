# Deployment — Architecture

Last verified: 2026-05-10

## Purpose

The ATC Helm chart (`deploy/helm/atc/`) packages the ATC server for deployment to any
conformant Kubernetes cluster. It produces a single mandatory Deployment backed by a
ClusterIP Service and a dedicated ServiceAccount. Optional resources (Ingress, HTTPRoute,
ServiceMonitor, `helm test` hook) are each gated behind independent values flags that default
to `false`.

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

**Decision:** Metrics port always bound in the container; `metrics.enabled` gates only Service-level exposure
**Alternatives considered:** Conditional metrics listener based on chart flag; separate metrics Deployment
**Rationale:** The metrics listener is a backend concern — the binary always binds both ports. Gating the Service port exposure keeps Prometheus scraping optional without requiring chart-level changes to the container runtime behavior. This matches how CNCF projects (cert-manager, Linkerd) handle the same pattern.

**Decision:** Dual chart publishing channels — OCI (`oci://ghcr.io/bojanrajkovic/charts/atc`) and a classic HTTP repo on GitHub Pages (`https://bojanrajkovic.github.io/atc/charts`)
**Alternatives considered:** OCI only; GitHub Pages only
**Rationale:** OCI is the canonical channel for OCI-native workflows and is the only channel that carries the Sigstore build-provenance attestation. GitHub Pages is the recommended channel for consumers without GHCR authentication — `helm repo add` works against any laptop or CI without registry credentials. Both channels are tag-triggered from the same workflow and gated so the Pages publish only runs after the OCI publish succeeds; chart versions stay in lockstep. See `docs/architecture/release-pipeline.md` for the workflow shape and the manual `gh-pages` Pages-source prerequisite.

**Decision:** Dual routing support — optional Ingress (`networking.k8s.io/v1`) and optional HTTPRoute (`gateway.networking.k8s.io/v1`)
**Alternatives considered:** Ingress only; Gateway API only; neither (document port-forward)
**Rationale:** Ingress covers clusters with a classic ingress controller (nginx, traefik). HTTPRoute covers clusters running a Gateway API controller (Envoy Gateway, Cilium). Providing both optional templates at the same chart version avoids forking. The default is neither — port-forward instructions in NOTES.txt cover the zero-dependency case.

## Environment Variables (ATC_* surface)

The chart wires Helm values to ATC_* environment variables. The canonical list is in `backend/crates/atc-server/src/config.rs`. One optional variable is available beyond the core set:

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

## Multi-replica

`replicaCount > 1` requires a PostgreSQL connection string via either `config.databaseUrl` or `existingSecret.name`+`existingSecret.databaseUrlKey`. The chart enforces this at template-render time with a `{{ fail }}` guard at the top of `templates/deployment.yaml`. The same template also rejects any non-PostgreSQL scheme on the inline `config.databaseUrl` path (`postgres://` and `postgresql://` are the only accepted prefixes); the `existingSecret` path is opaque at render time and falls through to a startup-time scheme check in the binary (`ensure_pg_scheme()` in `backend/crates/atc-server/src/main.rs`) that exits with a remediation-naming log line before any sqlx connect call. The chart's ephemeral in-memory mode is single-replica only.

The runtime invariants that make symmetric multi-replica safe (see [`state-externalization-research/`](state-externalization-research/README.md)):

- **Per-replica `broadcast_watermark`.** Each replica owns an `Arc<AtomicI64>` cursor advanced by the drain task only after a successful drain pass. `/v1/state` snapshot reads do an `Acquire` load before opening the snapshot transaction; the snapshot's `lastSeq` is bounded by what that replica has actually broadcast.
- **REPEATABLE READ snapshots.** `/v1/state` opens a REPEATABLE READ transaction that reads `runs`, `jobs`, and `MAX(outbox.seq)` from one MVCC snapshot. Without this isolation level, a concurrent webhook commit between the runs SELECT and the seq SELECT could advance `lastSeq` past content the snapshot hasn't materialized — the frontend's `seq > lastSeq` filter would then permanently drop a real event.
- **Ring-buffer dedup (single-delivery contract).** The drain task carries a 2048-seq ring buffer (~16 KB per replica) and skips a NOTIFY-driven row if its seq has already been broadcast on this replica. This preserves ADR 0003's no-frontend-dedup stance under gap-healing rescans (a re-fetched row never reaches the WS broadcast a second time).
- **Drain heartbeat readiness.** `/readyz` 503s when the drain heartbeat is older than 30s, and short-circuits to 503 `{"status":"shutting_down"}` once `state.shutdown` is cancelled (see § Graceful shutdown below). Routes a misbehaving or terminating replica out of Service endpoints fast enough that a client reconnect lands on a healthy peer.

**Sticky sessions are NOT required** and are discouraged outside specific cost-tuning scenarios. Reconnect-then-snapshot via `/v1/state`+`lastSeq` is the design (ADR 0002 D5). A client that always lands on the same replica via sticky cookies will never exercise the reconnect-across-replicas code path, masking gap-healing regressions in development. Operators with specific needs (e.g., reducing reconnect storms during rolling updates) can add sticky-cookie annotations themselves at the Ingress / HTTPRoute level — the chart does not do this by default.

**Anti-affinity / PDB / HPA defaults are not provided.** Tracked as #10 / #9 / #8.

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

# 7. Each WS-tap logfile must show exactly one SeqEvent for the webhook
# (single-delivery via ring-buffer dedup). Allow up to 2s for live delivery.
sleep 2
COUNT_A=$(grep -c '"seq":' /tmp/ws-a.log 2>/dev/null || true)
COUNT_B=$(grep -c '"seq":' /tmp/ws-b.log 2>/dev/null || true)
COUNT_A=${COUNT_A:-0}
COUNT_B=${COUNT_B:-0}
if (( COUNT_A != 1 )) || (( COUNT_B != 1 )); then
  echo "FAIL: expected exactly one SeqEvent per replica (A=$COUNT_A, B=$COUNT_B)" >&2
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
- Each WS-tap logfile shows exactly one `SeqEvent` for the webhook.
- Both `/readyz` endpoints return 200 throughout the test.

`kubectl logs -l app.kubernetes.io/name=atc -f --prefix` tags each line with the pod name — sufficient for "which replica did what" attribution during inspection. Per-process replica identification at the metrics layer is provided by Prometheus's standard scrape-injected target labels (`pod`, `instance`) — the `atc_pg_*` metrics ship unlabeled per-process and dashboards aggregate `by (pod)`. See `docs/architecture/metrics.md` § Operational metrics for the per-metric scoping rules.

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

**Owns:** Kubernetes resource templates (Deployment, Service, ServiceAccount, and optional Ingress/HTTPRoute/ServiceMonitor), values schema validation, post-install operator guidance (NOTES.txt), chart packaging, and chart publishing on the OCI and GitHub Pages channels
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
- `deploy/helm/atc/templates/service.yaml` — ClusterIP Service with `http` port always present and `metrics` port gated on `metrics.enabled`
- `deploy/helm/atc/templates/serviceaccount.yaml` — ServiceAccount gated on `serviceAccount.create`; `automountServiceAccountToken: false`
- `deploy/helm/atc/templates/NOTES.txt` — Post-install guidance with conditional ingress/gateway/port-forward branches and plain-credentials warning
- `deploy/helm/atc/templates/ingress.yaml` — Optional Ingress (`networking.k8s.io/v1`), gated on `ingress.enabled`; supports TLS, hosts, and custom annotations
- `deploy/helm/atc/templates/httproute.yaml` — Optional HTTPRoute (`gateway.networking.k8s.io/v1`), gated on `gateway.enabled`; validates non-empty `parentRefs` via `{{ fail }}` guard
- `deploy/helm/atc/templates/servicemonitor.yaml` — Optional ServiceMonitor (`monitoring.coreos.com/v1`), gated on `metrics.enabled && metrics.serviceMonitor.enabled`; includes label selector for Prometheus discovery
- `deploy/helm/atc/templates/tests/test-connection.yaml` — Helm test hook Pod with restricted Pod Security Standards; validates Service connectivity; excluded from charts via `helm.sh/hook: test` annotation
- `deploy/helm/atc/tests/values-*.yaml` — CI values matrix (defaults, ingress, gateway, multi-replica, metrics) feeding `helm template | kubeconform`; excluded from chart tarball by `.helmignore /tests/` anchor

## Storage modes

The chart supports two storage modes — ephemeral in-memory and external Postgres — per ADR 0003 D3. SQLite was considered and rejected:

- **SQLite not supported.** SQLite has no `LISTEN/NOTIFY` equivalent. Supporting it as a single-replica durable mode would require dual SQL flavors with different forwarder implementations (Postgres push, SQLite poll). The maintenance and test-matrix cost of dual SQL backends outweighs the value of "single-binary + PVC durable mode" as a deployment shape.
- **No `persistence.*` chart machinery.** The chart has no PVC template, `persistence:` values block, or persistence-conditional volume mounts. An audit found no application-code consumer of Kubernetes PVCs (only in-memory state, sessionStorage/localStorage in the frontend, and the PostgreSQL layer). With zero current or planned consumers, a templated PVC would be dead code.
- **Constant `RollingUpdate` strategy.** Both supported modes are RWO-volume-free, so a constant `RollingUpdate` (`maxSurge: 1, maxUnavailable: 0`) gives zero-downtime in both.
- **Multi-replica precondition guard.** A template-render-time `{{ fail }}` guard rejects `replicaCount > 1` without a Postgres URL (via either `config.databaseUrl` or `existingSecret`).

Operators whose values files contain a `persistence:` key will see schema validation reject the unknown property (`additionalProperties: false`). Mitigation: remove the `persistence:` block from operator values files. There is no programmatic migration tool — this is a deliberate breaking change in a 0.x chart.

If a future use case requires PVC-backed storage (e.g., a sidecar buffering audit logs to disk), the surface should be re-introduced tightly scoped to that consumer rather than as a general-purpose toggle.
