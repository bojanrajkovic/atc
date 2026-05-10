# Graceful shutdown deploy surface — preStop hook and readiness probe coordination

> **Implementation note:** read `docs/implementation-guidance.md` before writing any code.

Tracks issue #79. Follow-up to issue #60 / PR #81 (cooperative pod-internal shutdown).

## Context

PR #81 (`4c8db6b`) landed cooperative pod-internal shutdown: a `tokio_util::sync::CancellationToken` is fanned out from `main.rs:174` through `AppState.shutdown` (`state.rs:107`) to five supervised surfaces — eviction loop, PG listener, drain task, metrics server, and per-connection WS handlers. The aggregate worst-case shutdown budget is ~13 s, leaving 17 s of headroom inside Kubernetes' default `terminationGracePeriodSeconds: 30`.

What that work intentionally did **not** address (deferred to this issue, called out in `docs/architecture/backend-server.md:454-462`):

1. **LB de-registration race.** When the kubelet sends SIGTERM, the Service load balancer's iptables/IPVS rules are eventually consistent. New webhook POSTs and WS upgrades can land on the dying pod for a few seconds. axum's `with_graceful_shutdown` accepts and immediately drains them, returning errors to GitHub (in-flight retries) and to browser clients.

2. **Readiness probe is unaware of shutdown.** `/readyz` (`routes.rs:56-88`) currently checks PG connectivity and drain heartbeat staleness, but does **not** observe `state.shutdown`.

### How Kubernetes drains traffic during pod termination

The lifecycle (per [Pod Termination](https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/) and [EndpointSlice](https://kubernetes.io/docs/concepts/services-networking/endpoint-slices/) docs) is:

1. **Pod is marked for deletion.** `metadata.deletionTimestamp` is set.
2. **EndpointSlice controller flips the endpoint condition** to `ready=false, serving=true, terminating=true` immediately (per KEP-1672 / `ProxyTerminatingEndpoints`, GA in K8s 1.28). kube-proxy and conformant load balancers stop sending **new** traffic to this endpoint, while in-flight requests continue to the still-running pod.
3. **kubelet runs the `preStop` hook.** This is what the chart change in this issue introduces: a sleep that absorbs the **propagation delay** between the EndpointSlice update and kube-proxy / cloud-LB rule sync (typically 1–2 s on healthy clusters; 5 s is conservative).
4. **kubelet sends SIGTERM.** axum's `shutdown_signal` task (`main.rs:57`) catches it, calls `shutdown.cancel()`, and the cooperative-shutdown orchestration runs (~13 s budget).
5. **kubelet sends SIGKILL** if `terminationGracePeriodSeconds` (30 s) elapses before the process exits.

**The drain mechanism is steps 2 + 3 — EndpointSlice flip + preStop sleep — not the `/readyz` 503.** A previous draft of this plan claimed `/readyz` returning 503 "accelerates LB removal." That is incorrect on K8s 1.28+ in the standard Service-routed case: the EndpointSlice already reads `ready=false` from `deletionTimestamp` regardless of probe state.

### Why the `/readyz` change still belongs in this issue

For in-cluster Service-routed traffic on K8s 1.28+, the `/readyz` 503 is **not** the primary drain mechanism — the EndpointSlice flip is. But several real deployment topologies route traffic through a separate control loop that watches readiness probes directly, and for those the `/readyz` flip is load-bearing:

- **Cloud load-balancer controllers with their own deregistration loop.** AWS Load Balancer Controller (the IngressClass `alb` and Service `nlb-ip` modes), GCE NEG via `cloud.google.com/neg`, and Azure Application Gateway Ingress Controller all bypass kube-proxy and register pod IPs directly into the cloud LB's target group. Each provides a **pod-readiness gate** (`target-health.elbv2.k8s.aws/<ARN>` for AWS) and uses the kubelet's reported pod readiness — derived from probes — to decide when to mark a target as `draining` via the cloud API. The cloud-side deregistration delay (default 300 s for AWS ALB, configurable but typically minutes-not-seconds) is much slower than EndpointSlice propagation, so the readiness flip is what starts the cloud-side drain clock. Without the `/readyz` change, the cloud LB only learns the pod is gone when the kubelet stops responding to probes after SIGKILL — far too late.
- **Service meshes that read readiness probes directly** (Istio, Linkerd in some configs) need the probe flip to remove the pod from the mesh's own routing.
- **External health monitors / synthetic probes** that hit `/readyz` from outside the cluster see the shutdown as it happens instead of as a connection error after `with_graceful_shutdown` closes the listener.
- **Operators reading `kubectl describe pod` during a rolling update** see `Readiness: shutting_down` instead of an ambiguous "Endpoints removed it but probe last said OK."
- **Traffic landing on the pod between SIGTERM and listener close** gets a fast-fail 503 with a meaningful body (`{"status":"shutting_down"}`) instead of either `db_unreachable` (false) or a successful 200 (misleading).

The two changes — chart-side preStop+grace-period and backend-side `/readyz` shutdown awareness — compose to deliver the operator-visible "no in-flight error during rolling update; pod's intent is observable from in-cluster, in-mesh, AND in-cloud-LB control planes" property.

## Definition of Done

1. `/readyz` returns 503 with body `{"status":"shutting_down"}` once `state.shutdown` has been cancelled, in addition to the existing 503 reasons.
2. `/healthz` continues to return 200 unconditionally (including during shutdown). The kubelet liveness check must not restart the pod mid-drain.
3. The Helm chart renders a `lifecycle.preStop.sleep` block on the app container by default, gated by a values knob that allows opt-out.
4. The Helm chart renders `terminationGracePeriodSeconds` on the pod spec, defaulting to 30, exposed as a values knob.
5. `deploy/helm/atc/values.schema.json` accepts the new keys; `helm lint` passes.
6. New helm-unit tests cover the conditional rendering of the preStop block (default-on, opt-out by `0`) and the `terminationGracePeriodSeconds` value flowing through.
7. The existing `backend/crates/atc-server/tests/integration/readyz.rs` is extended with tests for the shutdown path (`/readyz` → 503, `/healthz` → 200 on the same cancelled `AppState`).
8. `docs/architecture/deployment.md`, `docs/architecture/backend-server.md`, and `deploy/helm/atc/CLAUDE.md` reflect the new contract; `docs/architecture/backend-server.md:462` no longer describes issue #79 as future work.

## Locked Decisions

Established in prior phases — not open for re-evaluation:

- **Shutdown signaling primitive.** Single `tokio_util::sync::CancellationToken`, fanned out through `AppState.shutdown`. Established by `docs/design-plans/2026-05-09-supervision-and-shutdown.md`. New code observes `state.shutdown.is_cancelled()`; we do not introduce a parallel `AtomicBool`.
- **Aggregate shutdown budget ~13 s.** Established by `backend/crates/atc-server/src/shutdown.rs:24`. The 30 s `terminationGracePeriodSeconds` default below is calibrated against this; do not lower it.
- **Liveness vs readiness split.** `/healthz` is unconditionally 200 (liveness — kubelet must not restart the pod during shutdown); `/readyz` is the surface that signals shutdown. Established at `routes.rs:52-88`.
- **`/readyz` body shape.** Existing reasons are `ok | db_unreachable | drain_stale` returned via the `HealthResponse { status: &'static str }` struct. The new reason `shutting_down` slots into this enum-by-string convention; we do not restructure the body.
- **Integration test consolidation.** All `atc-server` integration tests live in a single binary at `backend/crates/atc-server/tests/integration/main.rs`, established by PR #83 (`3a54d44`). New tests extend existing modules — they do not add top-level `tests/*.rs` files.

## Architecture

### Backend — `/readyz` shutdown awareness

**Decision.** Insert a `state.shutdown.is_cancelled()` check at the top of `readyz()` in `backend/crates/atc-server/src/routes.rs`, returning `(503, {"status":"shutting_down"})` when true. The check runs before the PG and drain-heartbeat checks so a shutting-down pod stops doing PG work on every probe pass.

```rust
async fn readyz(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if state.shutdown.is_cancelled() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(HealthResponse { status: "shutting_down" }),
        )
            .into_response();
    }
    // existing PG + drain-heartbeat checks unchanged
    // ...
}
```

`/healthz` (`routes.rs:52`) is unchanged — unconditional 200.

**Why `is_cancelled()` synchronously, not `tokio::select!` on `cancelled().await`.** The probe path is short-circuit: we want a synchronous `bool` peek, not an async race. `CancellationToken::is_cancelled()` is a non-blocking atomic load — appropriate for a hot probe path called every few seconds.

**Why the shutdown check goes first.**
- Cheaper than a PG round-trip.
- During shutdown the PG pool may already be draining; we don't want spurious `db_unreachable` responses to mask the real reason.
- Matches the operator's mental model: "the pod is going away" is a stronger statement than "PG is sad."

**Rejected alternatives.**

- *Add a separate `/shutdown` endpoint and switch the probe.* Two endpoints to coordinate; doesn't compose with the existing `db_unreachable | drain_stale` failure modes; harder to reason about ordering of helm changes vs backend changes.
- *Add an `Arc<AtomicBool>` set from `run_shutdown_orchestration`.* Parallel state for the same fact `state.shutdown` already encodes. The token is the single source of truth — a duplicate flag invites drift.

### Helm — `preStop` lifecycle hook (native K8s `sleep` action)

**Decision.** Use Kubernetes' native `lifecycle.preStop.sleep` action (KEP-3960, beta default-on in 1.30, GA in 1.33). The chart already requires `kubeVersion: ">=1.29.0-0"`; this issue **bumps it to `">=1.32.0-0"`** so the sleep action is available default-on AND so the chart's minimum is still a maintained K8s release.

```yaml
{{- if gt (.Values.shutdown.preStopSleepSeconds | int) 0 }}
lifecycle:
  preStop:
    sleep:
      seconds: {{ .Values.shutdown.preStopSleepSeconds }}
{{- end }}
```

**Why native, not `exec ["/bin/sleep", "N"]`.** The runtime base image is `gcr.io/distroless/cc-debian13:nonroot` (`Dockerfile:62`). The Distroless `cc` variant ships only the C runtime libs and the application binary; **there is no `/bin/sleep`, no shell, and no busybox** ([Distroless README](https://github.com/GoogleContainerTools/distroless)). An `exec` preStop with `sleep` would fail with `ENOENT` at runtime, leaving the chart silently shipping a broken hook. The native action is implemented by the kubelet itself — the container image needs nothing.

**Why bump `kubeVersion` to `>=1.32.0-0`.** `PodLifecycleSleepAction` timeline (per [KEP-3960](https://github.com/kubernetes/enhancements/issues/3960) and the [K8s v1.33 lifecycle blog](https://kubernetes.io/blog/2025/05/14/kubernetes-v1-33-updates-to-container-lifecycle/)):

- 1.29: alpha, feature gate **off** by default — requires explicit enablement.
- 1.30–1.32: beta, feature gate default-**on**.
- 1.33: GA — the gate is removed.

Technical minimum is `>=1.30.0-0`; we bump to `>=1.32.0-0` instead because 1.30 and 1.31 are EOL by May 2026, and choosing the most-recent-still-maintained beta version reduces the long tail of "default-on but never tested at scale" surface. ATC is at `0.1.0` (pre-stable); a `kubeVersion` bump pre-1.0 is appropriate. Operators on EOL clusters should pin an earlier chart version.

**Why default-on at 5 s.** EndpointSlice propagation latency is dominated by:
- pod transition → API server: <100 ms
- EndpointSlice controller reconcile: 100–500 ms typical
- kube-proxy iptables/IPVS sync: bounded by `--iptables-min-sync-period=1s` (default), so changes propagate within ~1–2 s on most clusters

5 s is conservative for typical clusters; operators on high-cadence clusters can lower to `2`, and operators on slow clusters (e.g., large IPVS tables) may bump to `10`. Default-on prevents the silent footgun of "we shipped graceful shutdown but the LB didn't know."

**Validator constraint.** The K8s API validator rejects `Sleep.Seconds <= 0`, so the conditional gate (omit the `lifecycle` block when `preStopSleepSeconds == 0`) is required for the opt-out path, not optional.

**Rejected alternatives.**

- *Stick with `exec ["/bin/sleep", "N"]` and add `coreutils` or `busybox` to the runtime image.* Adds image size, surfaces a shell where the security model assumes none, and re-creates the very class of supply-chain footgun distroless was chosen to avoid.
- *Inject the sleep inside the binary (Rust `tokio::time::sleep` between SIGTERM receipt and shutdown).* This conflates two distinct delays. The preStop sleep happens *before* SIGTERM (preserving in-flight requests to a still-listening server); a post-SIGTERM sleep would have the server stop accepting first. The LB de-registration race needs the pre-SIGTERM variant.
- *Use `httpGet` preStop calling `/readyz`.* The preStop hook completes when the request returns; it doesn't wait for anything to drain. Useless for absorbing kube-proxy propagation delay.

### Helm — `terminationGracePeriodSeconds`

**Decision.** Render `terminationGracePeriodSeconds` from `.Values.shutdown.terminationGracePeriodSeconds`, defaulting to `30`. Required range is `>= preStopSleepSeconds + 13` (the in-process budget); we do **not** add a chart-render guard, but we do call out the relationship in `values.yaml` comments and `docs/architecture/deployment.md`. The default 30 satisfies this for the default 5 s preStop (5 + 13 = 18 < 30).

**Why no fail-guard on the relationship.** Helm's `fail` is for invariants the chart cannot recover from — multi-replica without external PG (`templates/deployment.yaml`) is the existing precedent. The preStop / shutdown relationship is a soft constraint: setting `terminationGracePeriodSeconds: 20` with a `preStopSleepSeconds: 30` would be operationally bad but produces a renderable manifest. Document the relationship; trust the operator.

### Helm — values structure

**Decision.** Group both knobs under a top-level `shutdown:` map in `values.yaml`, and add a matching `shutdown` property to `values.schema.json` with `additionalProperties: false` and minimums.

`values.yaml`:

```yaml
# Pod lifecycle for graceful shutdown. Pairs with the in-process shutdown
# orchestration in atc-server (~13 s aggregate budget). See
# docs/architecture/deployment.md § Graceful shutdown.
shutdown:
  # Seconds the preStop hook sleeps before SIGTERM is delivered. Absorbs the
  # propagation delay between the EndpointSlice flipping ready=false and
  # kube-proxy / cloud-LB rule sync. Set to 0 to disable the hook.
  # Implemented via the kubelet-native preStop sleep action (K8s 1.32+).
  preStopSleepSeconds: 5

  # Pod-spec terminationGracePeriodSeconds. Must be >= preStopSleepSeconds + 13
  # (the worst-case in-process shutdown budget). Default 30 covers the default
  # 5 s preStop with comfortable headroom.
  terminationGracePeriodSeconds: 30
```

`values.schema.json` addition (sketch — exact placement matches the file's existing alphabetic-ish ordering):

```json
"shutdown": {
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "preStopSleepSeconds": {
      "type": "integer",
      "minimum": 0,
      "description": "Seconds the preStop hook sleeps before SIGTERM. 0 disables the hook."
    },
    "terminationGracePeriodSeconds": {
      "type": "integer",
      "minimum": 1,
      "description": "Pod-spec terminationGracePeriodSeconds. Should be >= preStopSleepSeconds + 13."
    }
  }
}
```

The schema's existing `additionalProperties: false` (root, line 4) means rendering any chart with new values keys would fail `helm lint` until the schema is updated; this is required.

**Rejected alternative — flat at root.** `terminationGracePeriodSeconds: 30` and `preStop: { sleepSeconds: 5 }` at root level is more idiomatic for some charts but loses the semantic grouping. The two knobs are coupled (the comment in (1) cites (2)); colocating them under `shutdown:` makes the relationship discoverable on `helm show values`.

### Test surface

**Backend integration tests (extend `tests/integration/readyz.rs`).** The existing module already builds `AppState` directly and exercises `/readyz` via `tower::ServiceExt::oneshot` (see the `stale_heartbeat_returns_503` test pattern). Add two new tests there:

1. **`shutdown_cancelled_returns_503`** — build `AppState` with a fresh `CancellationToken`, fire `state.shutdown.cancel()`, send `GET /readyz`, assert 503 and `{"status":"shutting_down"}`.
2. **`healthz_returns_200_after_shutdown`** — same setup; send `GET /healthz`; assert 200 and `{"status":"ok"}`. This locks in the liveness/readiness split AC2 demands.

Neither requires PG (the shutdown check short-circuits before any PG access), so neither needs Docker.

**Helm-unit tests.** New `deploy/helm/atc/tests/unit/shutdown.yaml`. Three scopes per `feedback_helm_test_scope.md`:

1. **Conditional emission.** With `shutdown.preStopSleepSeconds: 5` (default), the rendered container has `lifecycle.preStop.sleep.seconds: 5`. With `shutdown.preStopSleepSeconds: 0`, the container has no `lifecycle` block.
2. **`terminationGracePeriodSeconds` flow-through.** A non-default `terminationGracePeriodSeconds: 60` renders on the pod spec.
3. **Custom sleep duration.** With `shutdown.preStopSleepSeconds: 10`, the rendered `sleep.seconds` is `10`.

Tautological "default value renders default" assertions are skipped per `feedback_helm_test_scope.md`. The branch tests above ARE testing invariants (the conditional gating and template substitution).

## Implementation Phases

TDD-ordered, paired red→green per surface (per `docs/planning-workflow.md` "step 2 = make them pass"):

### 1. Backend: write failing `/readyz` shutdown test (and `/healthz` stays-200 test)

Edit `backend/crates/atc-server/tests/integration/readyz.rs` to add `shutdown_cancelled_returns_503` and `healthz_returns_200_after_shutdown`. Run `mise exec -- cargo nextest run -p atc-server readyz` and confirm the new tests fail because `readyz` ignores the shutdown token. (`/healthz` may already pass — that's fine; the test locks in the invariant.)

### 2. Backend: implement `/readyz` shutdown awareness

Edit `backend/crates/atc-server/src/routes.rs:56` per the snippet in Architecture above. Re-run the tests in module `readyz` and confirm both new tests pass; existing tests (drain heartbeat, no-PG fallback) remain green.

### 3. Helm: write failing helm-unit tests

Create `deploy/helm/atc/tests/unit/shutdown.yaml` with the three scopes above. Run `mise exec -- helm unittest deploy/helm/atc` and confirm all three fail because the template doesn't render `lifecycle` or `terminationGracePeriodSeconds`. (Schema-related failures may or may not surface here depending on whether `helm unittest` runs schema validation; phase 4 makes it conclusive.)

### 4. Helm: implement chart changes (template + values + schema, in one phase)

Three edits, all required for `helm lint` to pass:

- `deploy/helm/atc/values.yaml` — add the `shutdown:` map with the two keys and the doc comments.
- `deploy/helm/atc/values.schema.json` — add the `shutdown` property block.
- `deploy/helm/atc/templates/deployment.yaml` — render the gated `lifecycle.preStop.sleep` block and the `terminationGracePeriodSeconds` field. Bump `Chart.yaml`'s `kubeVersion` to `">=1.32.0-0"`.

Re-run `mise exec -- helm unittest deploy/helm/atc` and confirm the new suite passes; re-run `mise exec -- helm lint deploy/helm/atc` and confirm clean.

### 5. Update architecture docs

- `docs/architecture/deployment.md` — add a "Graceful shutdown" subsection covering the K8s termination sequence (with EndpointSlice flip), preStop hook, terminationGracePeriodSeconds, the `shutdown.*` values, and the rolling-update timeline. Update the existing `/readyz` reference (line 91) to mention the third 503 reason. Cross-link to `backend-server.md § Operator shutdown contract`.
- `docs/architecture/backend-server.md` — replace the "tracked as a separate operational improvement (issue #79)" sentence (line 462) with the now-shipped contract; reference the chart values; add `shutting_down` to the documented `/readyz` response set (line 470).
- `deploy/helm/atc/CLAUDE.md` — append a one-paragraph "Graceful shutdown" pointer to `docs/architecture/deployment.md § Graceful shutdown`. No content duplication.
- All three: bump `Last verified`.

### 6. Local verification

Run the verification surface that matters for this change (note: the lefthook `pre-push` hook only runs `cargo nextest run --workspace`, `pnpm exec vitest run`, and `scripts/check-docs-lefthook.sh` — the helm and lint commands below are run ad-hoc):

- `just lint` — fmt, clippy, helm lint
- `just test` — backend tests including the two new readyz tests
- `mise exec -- helm unittest deploy/helm/atc` — including the new `shutdown.yaml` suite
- `mise exec -- helm lint deploy/helm/atc` — schema-validates `values.yaml` against `values.schema.json`
- Spot check: `mise exec -- helm template deploy/helm/atc --set shutdown.preStopSleepSeconds=0 | grep -c lifecycle` should print `0`; default render should show one `lifecycle:` block.

### 7. Open PR

PR title: `feat(helm): graceful shutdown deploy surface — preStop hook and readiness probe coordination`. PR body summarizes the two-axis change (chart + backend). Test plan posted as the **first comment** per `feedback_test_plans.md`.

## Acceptance Criteria

- **AC1.** `GET /readyz` after `state.shutdown.cancel()` returns HTTP 503 with body `{"status":"shutting_down"}`. Verified by `shutdown_cancelled_returns_503` in `tests/integration/readyz.rs`.
- **AC2.** `GET /healthz` after `state.shutdown.cancel()` continues to return HTTP 200 with `{"status":"ok"}`. Verified by `healthz_returns_200_after_shutdown` in `tests/integration/readyz.rs`. Locks the invariant that kubelet liveness checks do not restart the pod mid-drain.
- **AC3.** `GET /readyz` before `shutdown.cancel()` (with PG healthy + drain heartbeat fresh) returns HTTP 200 with `{"status":"ok"}`. Existing `readyz.rs` tests cover this; not regressed.
- **AC4.** `helm template deploy/helm/atc` (default values) renders `lifecycle.preStop.sleep.seconds: 5` on the app container. Verified by new helm-unit test.
- **AC5.** `helm template deploy/helm/atc --set shutdown.preStopSleepSeconds=0` renders no `lifecycle` block on the app container. Verified by new helm-unit test.
- **AC6.** `helm template deploy/helm/atc --set shutdown.preStopSleepSeconds=10` renders `lifecycle.preStop.sleep.seconds: 10`. Verified by new helm-unit test.
- **AC7.** `helm template deploy/helm/atc --set shutdown.terminationGracePeriodSeconds=60` renders `terminationGracePeriodSeconds: 60` on the pod spec. Verified by new helm-unit test.
- **AC8.** `helm lint deploy/helm/atc` passes against default values and against `--set shutdown.preStopSleepSeconds=0` (schema accepts both shapes). Verified by Phase 6 spot-check; failure path is the schema rejecting `shutdown` as an unknown key.
- **AC9.** `docs/architecture/deployment.md` contains a "Graceful shutdown" subsection covering preStop, terminationGracePeriodSeconds, the values knobs, and the K8s termination timeline (including the EndpointSlice flip). Verified by hand-review.
- **AC10.** `docs/architecture/backend-server.md` no longer describes issue #79 as future work — the line at the existing reference (currently line 462) reflects the shipped chart contract and cross-links to `deployment.md § Graceful shutdown`. Verified by hand-review (semantic, not exact-string).
- **AC11.** Local verification suite passes: `just lint`, `just test`, `helm unittest deploy/helm/atc`, `helm lint deploy/helm/atc`, plus the lefthook `pre-push` hook (`cargo nextest run --workspace`, `pnpm exec vitest run`, `scripts/check-docs-lefthook.sh`) per `lefthook.yml`.

## Documents to Update

| Doc | Change |
|---|---|
| `docs/architecture/deployment.md` | Add "Graceful shutdown" subsection: K8s termination sequence (EndpointSlice flip + preStop + SIGTERM), values knobs, rolling-update timeline. Update existing `/readyz` reference (line 91) to mention the third 503 reason. Bump `Last verified`. |
| `docs/architecture/backend-server.md` | Replace the "tracked as a separate operational improvement (issue #79)" sentence (line 462) with the shipped contract; cross-link to `deployment.md § Graceful shutdown`. Add `shutting_down` to the documented `/readyz` response set (line 470). Bump `Last verified`. |
| `deploy/helm/atc/CLAUDE.md` | Append a one-paragraph "Graceful shutdown" pointer to `docs/architecture/deployment.md § Graceful shutdown`. Bump `Last verified`. |
| `deploy/helm/atc/values.schema.json` | Add `shutdown` property block (`additionalProperties: false`, integer minimums) so `helm lint` accepts the new keys. |
| `deploy/helm/atc/Chart.yaml` | Bump `kubeVersion` from `">=1.29.0-0"` to `">=1.32.0-0"` (native preStop sleep action requirement). |
| `scripts/doc-mapping.sh` | No new mapping needed. Existing entries cover both edit surfaces: `deploy/helm/atc/*` → `docs/architecture/deployment.md` (line 71); `backend/crates/atc-server/src/*` wildcard → `docs/architecture/backend-server.md` (line 47). Verified by reading the file. |

## Implementation Guidance

Rules from `docs/implementation-guidance.md` that bite for this scope (call out by name, do not relegate to a footer):

- **Read the design plan before coding.** This is the plan; implementation should not redesign during execution.
- **Run `just setup` at session start.** Per `feedback_verify_lefthook_installed.md` — missing hooks have caused repeated CI formatting failures on this repo.
- **Use `just test` or `cargo nextest run`, not bare `cargo test`.** Per `feedback_use_just_test_or_nextest.md`. For the focused dev loop on the new tests: `cargo nextest run -p atc-server readyz`.
- **Frontend E2E suite is not required.** This change does not touch the wire contract (`/readyz` body changes only add a new value to a non-`#[ts(export)]` struct), so `just test-e2e` is not required.

Project-memory feedback files that bite:

- `feedback_helm_test_scope.md` — new helm tests cover conditional behavior, not "field equals default value" tautologies.
- `feedback_no_source_grep_tests.md` — backend test verifies behavior (HTTP status + body), not source-text regex.
- `feedback_pr_title_convention.md` — PR title reflects the full deliverable, not just the first commit.
- `feedback_test_plans.md` — test plan goes in the first PR comment, not in the PR body or a committed file.
- `feedback_phases_not_in_user_facing_strings.md` — implementation-time code/docs do not carry "Phase N" / "AC N" labels per `CONTRIBUTING.md § Planning-Artifact Labels`. The plan file itself uses ACx labels (the plan IS a planning artifact); the architecture docs and code do not.
- `feedback_no_pip_install_in_agents.md` — the `helm unittest` plugin is provisioned via mise (`.mise.toml`); do not `npm install -g`, `pip install`, or otherwise pollute system tooling during verification.

## Out of Scope

- **Probe interval / threshold tuning.** `livenessProbe.periodSeconds`, `readinessProbe.failureThreshold`, etc. are not exposed as values knobs in this issue. Tracked separately if an operator hits a real-world need; default Kubernetes behavior (10 s period, 3-failure threshold) composes well with the 5 s preStop and 30 s grace period.
- **K8s 1.29–1.31 support.** The `kubeVersion` bump to `">=1.32.0-0"` drops these versions. They are EOL by May 2026; users still on them should pin an earlier chart version.
- **Service-mesh shutdown ordering.** Istio/Linkerd sidecars have their own shutdown sequencing; explicitly named out-of-scope in the issue.
- **Multi-cluster / cross-region failover.** Out of scope per the issue.
- **Webhook-side retry coordination.** GitHub webhook delivery has its own retry policy. Beyond returning fast 503 from a draining replica (which a healthy peer will pick up via the LB), no retry semantics change in this issue.

## Glossary

- **EndpointSlice.** Kubernetes object listing the IPs backing a Service. Updated by the EndpointSlice controller when pod readiness or termination changes; kube-proxy reads it to update iptables/IPVS rules.
- **`ProxyTerminatingEndpoints`.** Kubernetes feature (KEP-1672, GA in 1.28) that flips an endpoint to `ready=false, serving=true, terminating=true` as soon as the pod is marked for deletion — independent of probe state. The primary mechanism for stopping new traffic during pod termination.
- **`PodLifecycleSleepAction`.** Kubernetes feature (KEP-3960, beta default-on in 1.30, GA in 1.33) that lets `lifecycle.preStop` and `lifecycle.postStart` use a `sleep: { seconds: N }` action implemented by the kubelet — no in-container `sleep` binary required.
- **Cooperative shutdown budget.** The aggregate worst-case time between `shutdown.cancel()` firing and all supervised tasks completing. ~13 s in atc-server today, established by PR #81 (`shutdown.rs:24`).
