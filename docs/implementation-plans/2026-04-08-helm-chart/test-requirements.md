# Test Requirements — 2026-04-08-helm-chart

Maps every acceptance criterion sub-case (helm-chart.AC1.1 through helm-chart.AC11.5) to an automated test or a human-verification procedure. Scoped IDs use the `helm-chart.ACN.M` prefix.

---

## AC1 — Backend configuration via figment + `ATC_*` env prefix

| ID | Type | Test file or verification steps | Assertion summary | Phase |
|---|---|---|---|---|
| helm-chart.AC1.1 | auto (integration) | `backend/crates/atc-server/tests/config_tests.rs` — `Config::load()` with cleared `ATC_*` env | Defaults: `http_addr=0.0.0.0:8080`, `metrics_addr=0.0.0.0:9090`, `database_url=None`, `log_filter="info"`, `log_format` matches `cfg!(debug_assertions)` | 1 |
| helm-chart.AC1.2 | auto (integration) | `backend/crates/atc-server/tests/config_tests.rs` — env override test | `ATC_HTTP_ADDR=127.0.0.1:9999`, `ATC_METRICS_ADDR=127.0.0.1:7777`, `ATC_DATABASE_URL=sqlite::memory:` are applied to the loaded Config | 1 |
| helm-chart.AC1.3 | auto (integration) | `backend/crates/atc-server/tests/config_tests.rs` — malformed addr test | `ATC_HTTP_ADDR=not-a-socket-addr` causes `Config::load()` to return `Err` whose Display identifies the failing field | 1 |

## AC2 — Metrics side-port listener and `build_info` gauge

| ID | Type | Test file or verification steps | Assertion summary | Phase |
|---|---|---|---|---|
| helm-chart.AC2.1 | auto (integration) | `backend/crates/atc-server/tests/metrics.rs` | Metrics listener binds on ephemeral port and responds to `/metrics` request | 2 |
| helm-chart.AC2.2 | auto (integration) | `backend/crates/atc-server/tests/metrics.rs` | Response Content-Type starts with `text/plain; version=0.0.4`; body contains `atc_build_info{` with expected labels | 2 |
| helm-chart.AC2.3 | auto (integration) | `backend/crates/atc-server/tests/metrics.rs` | Body contains `process_cpu_seconds_total`, `process_resident_memory_bytes`, `process_open_fds`, `process_start_time_seconds` | 2 |
| helm-chart.AC2.4 | auto (integration) | `backend/crates/atc-server/tests/metrics.rs` | After oneshot requests to `/healthz` and `/readyz`, body contains `axum_http_requests_total` and `axum_http_requests_duration_seconds_bucket` | 2 |
| helm-chart.AC2.5 | human | Run `cargo run -p atc-server` twice concurrently (second instance fails to bind :9090); observe exit code non-zero and that main listener never binds | Metrics bind failure aborts startup before main listener opens; process exits non-zero | 2 |

Rationale for AC2.5 human verification: reliably simulating a port-in-use race inside a single Rust integration test (without flakiness and without leaving sockets in TIME_WAIT) is brittle; phase_02 establishes bind-order as the mechanism, which is trivial to verify interactively.

## AC3 — `/healthz` and `/readyz` routing

| ID | Type | Test file or verification steps | Assertion summary | Phase |
|---|---|---|---|---|
| helm-chart.AC3.1 | auto (integration) | `backend/crates/atc-server/tests/routes_tests.rs` — `healthz_returns_ok` via `tower::ServiceExt::oneshot` | `GET /healthz` returns 200, body `{"status":"ok"}`, `Content-Type: application/json` | 1 |
| helm-chart.AC3.2 | auto (integration) | `backend/crates/atc-server/tests/routes_tests.rs` — `readyz_returns_ok` | `GET /readyz` returns 200, body `{"status":"ok"}`, `Content-Type: application/json` | 1 |
| helm-chart.AC3.3 | auto (integration) | `backend/crates/atc-server/tests/routes_tests.rs` — `health_returns_404` | `GET /health` returns 404 (no backward-compat alias) | 1 |
| helm-chart.AC3.4 | auto (integration) | `backend/crates/atc-server/tests/metrics.rs` | Metrics router (side-port) has no `/healthz` or `/readyz` route; main router does | 1/2 |
| helm-chart.AC3.5 | auto (integration) | `backend/crates/atc-server/tests/metrics.rs` | `/metrics` body contains `/healthz` and `/readyz` path labels after oneshot requests | 2 |

## AC4 — Chart renders with default values

| ID | Type | Test file or verification steps | Assertion summary | Phase |
|---|---|---|---|---|
| helm-chart.AC4.1 | auto (ci-matrix) | CI helm job: `helm lint deploy/helm/atc` (also `just helm-lint`) | Exit 0, no errors or warnings | 3/5 |
| helm-chart.AC4.2 | auto (schema+diff) | `helm template ... --values tests/values-defaults.yaml` output verified by schema validation (kubeconform) and diff review; not field-value assertion | Renders only Deployment, Service, ServiceAccount, NOTES.txt, helm test Pod; no Ingress/HTTPRoute/PVC/ServiceMonitor | 4/5 |
| helm-chart.AC4.3 | auto (template-render) | `helm template ... --values tests/values-defaults.yaml \| grep -A3 strategy:` | Deployment has `strategy.type: RollingUpdate`, `maxSurge: 1`, `maxUnavailable: 0` | 4 |
| helm-chart.AC4.4 | auto (template-render) | `helm template ... --values tests/values-persistence.yaml` | Deployment has `strategy.type: Recreate`; PVC is rendered | 4 |
| helm-chart.AC4.5 | auto (template-render) | `helm template ... --set config.databaseUrl=sqlite:///var/lib/atc/atc.db` (CI negative step) | Exits non-zero with `{{ fail }}` message naming conflicting fields | 4 |
| helm-chart.AC4.6 | auto (template-render) | `helm template ... --values tests/values-persistence.yaml --set replicaCount=3` | Exits non-zero with `{{ fail }}` message naming conflicting fields | 4 |

## AC5 — Pod Security Standards (restricted) compliance

| ID | Type | Test file or verification steps | Assertion summary | Phase |
|---|---|---|---|---|
| helm-chart.AC5.1 | auto (template-render) | `helm template ... --values tests/values-defaults.yaml \| grep -A6 podSecurityContext` (or kubeconform matrix) | podSecurityContext has `runAsNonRoot: true`, `runAsUser/Group: 65532`, `fsGroup: 65532`, `seccompProfile.type: RuntimeDefault` | 3 |
| helm-chart.AC5.2 | auto (template-render) | `helm template ... \| grep -A8 'securityContext:'` on container | Container securityContext has `allowPrivilegeEscalation: false`, `readOnlyRootFilesystem: true`, `capabilities.drop: [ALL]`, `seccompProfile.type: RuntimeDefault` | 3 |
| helm-chart.AC5.3 | auto (template-render) | `helm template ... \| grep -A3 'name: tmp'` | Pod spec contains `tmp` emptyDir volume mounted at `/tmp` | 3 |
| helm-chart.AC5.4 | human | `kubectl label ns atc-pss-test pod-security.kubernetes.io/enforce=restricted`; `helm template ... \| kubectl apply --dry-run=server -n atc-pss-test -f -` | Admission accepts resources with no PSS warnings | 4 |
| helm-chart.AC5.5 | human | After `helm install` in a restricted namespace, run `helm test atc` | test-connection Pod admits and succeeds under restricted PSS | 4/6 |

Rationale for AC5.4/AC5.5 human verification: requires a live Kubernetes cluster with PSS enforcement, which CI does not run; phase_04 and phase_06 document this as the deferred live-cluster gate.

## AC6 — Logging format defaults and override

| ID | Type | Test file or verification steps | Assertion summary | Phase |
|---|---|---|---|---|
| helm-chart.AC6.1 | human | `cargo run -p atc-server`; observe terminal | Debug build emits ANSI-colored pretty-printed log lines | 1 |
| helm-chart.AC6.2 | human | `cargo build --release -p atc-server && ./backend/target/release/atc-server`; observe stdout | Release build emits single-line JSON with `timestamp`, `level`, `fields`, span context | 1 |
| helm-chart.AC6.3 | human | `ATC_LOG_FORMAT=pretty ./backend/target/release/atc-server` | Release binary emits pretty logs (override beats default) | 1 |
| helm-chart.AC6.4 | human | `ATC_LOG_FORMAT=json cargo run -p atc-server` | Debug build emits single-line JSON logs (override beats default) | 1 |

Rationale: phase_01 Task 8 explicitly documents AC6.1–AC6.4 as manual verification — spawning a child process, capturing stdout, and parsing log format is non-trivial and brittle; honoring that decision.

## AC7 — Optional templates render correctly

| ID | Type | Test file or verification steps | Assertion summary | Phase |
|---|---|---|---|---|
| helm-chart.AC7.1 | auto (schema+diff) | `helm template ... --values tests/values-ingress.yaml` output verified by schema validation (kubeconform) and diff review; not field-value assertion | Renders `networking.k8s.io/v1` Ingress with className, hosts, TLS | 4/5 |
| helm-chart.AC7.2 | auto (schema+diff) | `helm template ... --values tests/values-gateway.yaml` output verified by schema validation and diff review; not field-value assertion | Renders `gateway.networking.k8s.io/v1` HTTPRoute with parentRefs; empty `rules` yields default PathPrefix `/` to Service | 4/5 |
| helm-chart.AC7.3 | auto (helm-unittest) | `helm unittest` suite asserts PVC name invariant; PVC accessModes/size/storageClass verified by schema validation and diff review | Renders PVC with configured accessModes, size, storageClass; claimName matches Deployment volume reference | 4/5 |
| helm-chart.AC7.4 | auto (template-render) | `helm template ... --values tests/values-persistence.yaml --set persistence.existingClaim=my-claim` (inspect rendered YAML) | No PVC rendered; Deployment volume uses `claimName: my-claim` (schema+diff verification) | 4 |
| helm-chart.AC7.5 | auto (schema+diff) | `helm template ... --values tests/values-metrics.yaml` output verified by schema validation and diff review; not field-value assertion | Renders ServiceMonitor targeting `metrics` port with configured interval and scrapeTimeout | 4/5 |
| helm-chart.AC7.6 | auto (template-render) | `helm template ... --set metrics.serviceMonitor.enabled=true --set metrics.enabled=false` (inspect no ServiceMonitor in output) | No ServiceMonitor rendered when both flags required (schema+diff verification) | 4 |

Rationale for AC7 coverage approach: AC7.3 has a cross-template invariant assertion (PVC name matches Deployment claimName) via helm-unittest. AC7.1, AC7.2, AC7.4, AC7.5, AC7.6 verify schema correctness via kubeconform in the CI helm job matrix; the rendered YAML is inspected manually (or via diff tooling) to confirm optional templates are present/absent as expected. This avoids brittle field-value assertions on templates whose schema is stable and well-tested upstream.

## AC8 — CI helm job path filtering and matrix

| ID | Type | Test file or verification steps | Assertion summary | Phase |
|---|---|---|---|---|
| helm-chart.AC8.1 | human | Open a PR touching only `deploy/helm/atc/values.yaml`; observe CI | `helm` job runs; `backend`/`frontend` skipped; all three result gates green | 5 |
| helm-chart.AC8.2 | human | Open a PR touching only backend code; observe CI | `helm` job skipped; `helm-result` reports success via skipped-as-passed gate | 5 |
| helm-chart.AC8.3 | auto (ci-matrix) | `.github/workflows/ci.yml` helm job with `matrix: k8s × values` (2 × 5) | All 10 combinations run `helm template \| kubeconform -strict` successfully | 5 |
| helm-chart.AC8.4 | human | Introduce a deliberate `apiVersion: apps/v999` typo on a branch; push; observe `helm` job fail with kubeconform naming the invalid resource; revert | Broken template surfaces clear kubeconform error | 5 |
| helm-chart.AC8.5 | auto (ci-matrix) | `zizmor .github/workflows/ci.yml` (local and/or in zizmor.yml) | Zero findings | 5 |

Rationale: AC8.1/8.2/8.4 require an actual PR round-trip on GitHub to exercise `dorny/paths-filter` behavior and the `helm-result` gate — cannot be automated inside the repo's test suite.

## AC9 — Dockerfile runs as nonroot

| ID | Type | Test file or verification steps | Assertion summary | Phase |
|---|---|---|---|---|
| helm-chart.AC9.1 | human | `sudo nerdctl ... build -t atc:dev . && sudo nerdctl ... inspect atc:dev \| jq '.[0].Config.User'` | Reports `"65532:65532"` (distroless has no `id` binary) | 6 |
| helm-chart.AC9.2 | human | `sudo nerdctl ... run --rm -p 8080:8080 -p 9090:9090 atc:dev`; curl `/healthz`, `/readyz`, `/metrics` | Container starts with no USER override, both listeners bind, endpoints return 200 | 6 |
| helm-chart.AC9.3 | human | `helm install` into namespace labeled `pod-security.kubernetes.io/enforce=restricted`; pod becomes Ready | Pod admits and runs under restricted PSS with no chart-level `runAsUser` override | 6 |

Rationale: all AC9 sub-cases require building a container image on this host (nerdctl + k3s containerd) and/or a live Kubernetes cluster; neither is available in CI.

## AC10 — Chart published via release.yml with attestation

| ID | Type | Test file or verification steps | Assertion summary | Phase |
|---|---|---|---|---|
| helm-chart.AC10.1 | human | Tag a release in a test fork; observe `release.yml` run | OCI artifact appears at `oci://ghcr.io/<org>/charts/atc:<version>` after `build-container` succeeds | 6 |
| helm-chart.AC10.2 | human | `helm show chart oci://ghcr.io/<org>/charts/atc --version <version>` | Returns expected chart metadata (name, version, appVersion) | 6 |
| helm-chart.AC10.3 | human | `helm install atc oci://ghcr.io/<org>/charts/atc --version <version>` then `helm test atc` on a real cluster | Install succeeds; `helm test` exits zero | 6 |
| helm-chart.AC10.4 | human | `gh attestation verify ./dist/atc-*.tgz --owner <owner>` | Sigstore provenance verification succeeds | 6 |
| helm-chart.AC10.5 | human | In test fork, introduce deliberate `build-container` failure; re-run release.yml | `publish-helm-chart` is skipped; no chart artifact in registry | 6 |

Rationale: AC10 requires a real tag-triggered release run against a live OCI registry, a live cluster for install/test, and Sigstore verification — none of this is reachable from local or PR CI.

## AC11 — release-please integration

| ID | Type | Test file or verification steps | Assertion summary | Phase |
|---|---|---|---|---|
| helm-chart.AC11.1 | human | On a test branch, merge a synthetic `feat(helm): ...` commit; observe release-please run | PR bumps `deploy/helm/atc/Chart.yaml` version (minor) and updates `deploy/helm/atc/CHANGELOG.md` | 7 |
| helm-chart.AC11.2 | human | Inspect the release PR opened in AC11.1 | `sync-helm-app-version` job ran under bot identity; `Chart.yaml` `appVersion` matches `backend/crates/atc-server` in `.release-please-manifest.json`; commit author is `<slug>[bot]` | 7 |
| helm-chart.AC11.3 | human | Create scenario with only backend/frontend commits and observe release PR | `Chart.yaml` version unchanged; `appVersion` updated to new linked app version | 7 |
| helm-chart.AC11.4 | human | Observe release PR status checks | Backend, Frontend, Helm, and PR Checks status checks run on the release PR under bot identity | 7 |
| helm-chart.AC11.5 | human | Re-run `sync-helm-app-version` on the same release PR twice | Second run prints `appVersion already in sync; skipping commit`; exits zero; no new commit | 7 |

Rationale: phase_07 requires an actual release-please workflow execution with a GitHub App token and a real release PR — not reproducible offline.

---

## Human verification summary

Quick reference for the test-analyst agent: the following sub-cases cannot be automated and require manual verification as described above.

- helm-chart.AC2.5 — metrics bind-failure exit (port race)
- helm-chart.AC5.4 — `kubectl apply --dry-run=server` under PSS restricted
- helm-chart.AC5.5 — `helm test` hook under PSS restricted
- helm-chart.AC6.1 — debug build pretty logs (stdout inspection)
- helm-chart.AC6.2 — release build JSON logs (stdout inspection)
- helm-chart.AC6.3 — `ATC_LOG_FORMAT=pretty` on release binary
- helm-chart.AC6.4 — `ATC_LOG_FORMAT=json` on debug binary
- helm-chart.AC8.1 — helm-only PR path-filter behavior
- helm-chart.AC8.2 — backend-only PR skips helm job, gate passes
- helm-chart.AC8.4 — broken template surfaces kubeconform error in CI
- helm-chart.AC9.1 — container image runs as UID/GID 65532
- helm-chart.AC9.2 — container binds both listeners without USER override
- helm-chart.AC9.3 — container admits under restricted PSS namespace
- helm-chart.AC10.1 — tag-triggered OCI artifact publish
- helm-chart.AC10.2 — `helm show chart` against OCI artifact
- helm-chart.AC10.3 — `helm install` + `helm test` on real cluster
- helm-chart.AC10.4 — `gh attestation verify` Sigstore provenance
- helm-chart.AC10.5 — `publish-helm-chart` skipped on `build-container` failure
- helm-chart.AC11.1 — release-please opens chart-version PR on `feat(helm)` merge
- helm-chart.AC11.2 — `sync-helm-app-version` rewrites appVersion under bot identity
- helm-chart.AC11.3 — appVersion sync runs on app-only release
- helm-chart.AC11.4 — downstream CI runs on release PR under bot identity
- helm-chart.AC11.5 — `sync-helm-app-version` idempotent guard
