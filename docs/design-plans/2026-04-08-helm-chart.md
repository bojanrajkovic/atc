# Helm Chart Design

## Summary

Phase 6 of ATC ships a production-grade Helm chart at `deploy/helm/atc/` alongside the minimum backend changes required to deploy cleanly under Kubernetes' `restricted` Pod Security Standards. The work is sequenced in eight phases: the backend comes first (configuration, metrics, health endpoint rename), then the chart templates, then CI validation, then release publishing, and finally release-please integration. Each phase's outputs are the prerequisites for the next, so nothing lands in isolation.

Architecturally, the chart wraps a single Deployment with a ServiceAccount and Service as mandatory resources, and four optional templates (Ingress, HTTPRoute, PersistentVolumeClaim, ServiceMonitor) each gated behind a values flag that defaults to false. A `helm test` hook is also shipped, but always-on (no gating flag) per Helm convention — hook-annotated Pods only execute when `helm test <release>` is invoked, not on regular install, so a gate would add a knob with no runtime effect. The backend grows three new capabilities to meet the chart's requirements: a figment-based configuration layer reads all settings from `ATC_*` environment variables, a second TCP listener on port 9090 serves Prometheus metrics including a `build_info` gauge compiled in at build time via vergen, and the health endpoint is renamed to the Kubernetes-conventional `/healthz`/`/readyz` split. The container image flips to the `distroless :nonroot` tag (UID 65532) so the image itself asserts its identity and the chart's restricted security context has nothing to override. Versioning is a two-track system: the chart version bumps independently from conventional commits via a new release-please `helm` release-type entry, while a bot job keeps `Chart.yaml`'s `appVersion` field synchronized to the linked application version after each release.

## Definition of Done

A generic, publishable Helm chart lives at `deploy/helm/atc/` and deploys ATC to any conformant Kubernetes cluster (homelab or work). The chart passes `restricted` Pod Security Standards out of the box, references operator-provided Secrets by name, defaults all convenience features off (ingress, persistence, metrics, ServiceMonitor), and ships with a single database connection-string value that later phases will consume. The chart is lockstep-released to `oci://ghcr.io/bojanrajkovic/charts/atc` via the existing tag-triggered release workflow, with `appVersion` synchronized to the application version. CI validates every PR that touches `deploy/helm/` by running `helm lint`, rendering templates against a matrix of representative values files, and validating the rendered YAML against upstream Kubernetes schemas via kubeconform.

**In scope:**
- `deploy/helm/atc/` — `Chart.yaml`, `values.yaml`, `values.schema.json`, `templates/` (Deployment, Service, ServiceAccount, optional Ingress, optional PVC, optional ServiceMonitor, NOTES.txt, `_helpers.tpl`)
- Small `atc-server` additions: `/readyz` liveness/readiness split from `/health`; side-port metrics listener with baseline `build_info` + process metrics via `metrics-exporter-prometheus`
- CI job: `helm lint` + `helm template` matrix + kubeconform, gated on `deploy/helm/**` path filter
- Release pipeline: `helm package` + `helm push` to ghcr.io OCI, added to `release.yml` after binaries/container build
- release-please integration: chart version bumps from conventional commits; `appVersion` synced from the app lockstep version via a post-release-please job (mechanism to be finalized in brainstorming)
- `docs/architecture/deployment.md` (new) and `docs/architecture/backend-server.md` (updated for `/readyz` and metrics side port)
- GitHub issues tracking deferred work: HPA, PDB, NetworkPolicy, multi-replica, `ct install` on kind, chart-releaser + GitHub Pages distribution

**Out of scope (filed as issues):**
- HorizontalPodAutoscaler, PodDisruptionBudget, NetworkPolicy, multi-replica support
- `helm/chart-testing-action` (`ct install`) on ephemeral kind clusters in CI
- GitHub Pages chart repo (OCI publishing only for Phase 6)
- Any backend database implementation (deferred to Phase 7+)

**Success criteria:**
- `just lint` → `helm lint deploy/helm/atc` passes
- `helm template deploy/helm/atc --values <each matrix file>` renders valid YAML that kubeconform accepts against upstream k8s schemas
- `helm install --dry-run --debug deploy/helm/atc` succeeds with defaults and with each convenience flag toggled on
- A real `helm install` on a k8s cluster starts a Pod that passes liveness + readiness, exposes the stub `/metrics` endpoint serving `build_info`, and (with ingress enabled) is reachable end-to-end
- Tagging a release triggers `helm push` to `oci://ghcr.io/bojanrajkovic/charts/atc` alongside the existing binary and container artifacts
- `appVersion` in the published chart matches the released app version

**Resolved trade-offs:**
- Secure defaults over convenient defaults (ingress/PVC/metrics opt-in; restricted PSS always on)
- Chart version independent from app version, with `appVersion` synchronized via post-release-please automation
- OCI-only publishing for Phase 6; chart-releaser + Pages is deferred Future Work

**Technical unknowns to resolve in brainstorming:**
- Exact mechanism for syncing `Chart.yaml` `appVersion` from the lockstep app version (release-please post-hook job vs `extra-files` vs custom updater)
- Distroless `cc-debian13` default UID and whether ATC's runtime needs any writable paths beyond the PVC mount under read-only rootfs
- Gateway API adoption level — whether it's worth templating an HTTPRoute alongside Ingress
- Helm best practices verification for chart layout, values schema, NOTES.txt conventions

## Acceptance Criteria

### helm-chart.AC1: Backend configuration via figment + ATC_* env prefix
- **helm-chart.AC1.1 Success:** `Config::load()` returns defaults (`http_addr=0.0.0.0:8080`, `metrics_addr=0.0.0.0:9090`, `database_url=None`, `log_filter="info"`, `log_format` per debug/release) when no env vars are set
- **helm-chart.AC1.2 Success:** Setting `ATC_HTTP_ADDR=127.0.0.1:9999` overrides `http_addr`; setting `ATC_METRICS_ADDR=127.0.0.1:7777` overrides `metrics_addr`; setting `ATC_DATABASE_URL=sqlite::memory:` populates `database_url` as `Some(...)`
- **helm-chart.AC1.3 Failure:** A malformed `ATC_HTTP_ADDR=not-a-socket-addr` causes `Config::load()` to return an error and `main` to exit non-zero with a clear error message identifying the failed field

### helm-chart.AC2: Metrics side-port listener and build_info gauge
- **helm-chart.AC2.1 Success:** `cargo run` binds both `http_addr` and `metrics_addr` simultaneously and logs both listening addresses at startup
- **helm-chart.AC2.2 Success:** `curl http://localhost:9090/metrics` returns a Prometheus text-format body with Content-Type `text/plain; version=0.0.4; charset=utf-8` containing `atc_build_info{version="...",git_sha="...",rustc_version="...",build_timestamp="...",target_triple="..."} 1`
- **helm-chart.AC2.3 Success:** The same body contains `process_cpu_seconds_total`, `process_resident_memory_bytes`, `process_open_fds`, and `process_start_time_seconds` metrics
- **helm-chart.AC2.4 Success:** The same body contains `axum_http_requests_total` and `axum_http_requests_duration_seconds_bucket` histograms after at least one request has hit the main listener
- **helm-chart.AC2.5 Failure:** A bind failure on the metrics listener (e.g., port already in use) causes the process to exit non-zero; the main listener is not left running

### helm-chart.AC3: Routing: /healthz, /readyz, no /health alias
- **helm-chart.AC3.1 Success:** `GET /healthz` returns HTTP 200 with body `{"status":"ok"}` and Content-Type `application/json`
- **helm-chart.AC3.2 Success:** `GET /readyz` returns HTTP 200 with body `{"status":"ok"}` and Content-Type `application/json`
- **helm-chart.AC3.3 Failure:** `GET /health` returns HTTP 404 (no backward-compat alias)
- **helm-chart.AC3.4 Success:** Both `/healthz` and `/readyz` are exposed on `http_addr`, not on `metrics_addr`
- **helm-chart.AC3.5 Success:** Both endpoints show up as rows in the `axum_http_requests_total` metric with their respective paths as labels

### helm-chart.AC4: Chart renders with default values
- **helm-chart.AC4.1 Success:** `helm lint deploy/helm/atc` exits zero with no errors or warnings
- **helm-chart.AC4.2 Success:** `helm template atc deploy/helm/atc` renders Deployment, Service, ServiceAccount, and NOTES.txt without optional templates (no Ingress, no HTTPRoute, no PVC, no ServiceMonitor)
- **helm-chart.AC4.3 Success:** The rendered Deployment uses `strategy: RollingUpdate` with `maxSurge: 1, maxUnavailable: 0` when `persistence.enabled: false`
- **helm-chart.AC4.4 Success:** `helm template ... --set persistence.enabled=true --set config.databaseUrl=sqlite:///var/lib/atc/atc.db` renders the Deployment with `strategy: Recreate` and a PVC template
- **helm-chart.AC4.5 Failure:** `helm template ... --set config.databaseUrl=sqlite:///var/lib/atc/atc.db` (without `persistence.enabled=true`) fails with a `fail` template error message naming the conflicting fields
- **helm-chart.AC4.6 Failure:** `helm template ... --set persistence.enabled=true --set replicaCount=3` fails with a `fail` template error message naming the conflicting fields

### helm-chart.AC5: Pod Security Standards (restricted) compliance
- **helm-chart.AC5.1 Success:** The rendered Deployment includes `podSecurityContext` with `runAsNonRoot: true`, `runAsUser: 65532`, `runAsGroup: 65532`, `fsGroup: 65532`, `seccompProfile.type: RuntimeDefault`
- **helm-chart.AC5.2 Success:** The rendered container `securityContext` includes `allowPrivilegeEscalation: false`, `readOnlyRootFilesystem: true`, `capabilities.drop: [ALL]`, `seccompProfile.type: RuntimeDefault`
- **helm-chart.AC5.3 Success:** The rendered Pod spec includes a `tmp` emptyDir volume mounted at `/tmp` (required by `readOnlyRootFilesystem: true`)
- **helm-chart.AC5.4 Success:** `kubectl apply --dry-run=server` against a namespace with `pod-security.kubernetes.io/enforce: restricted` label accepts the rendered Deployment
- **helm-chart.AC5.5 Success:** The `helm test` hook Pod uses the same restricted security context and runs successfully in a restricted namespace

### helm-chart.AC6: Logging format defaults and override
- **helm-chart.AC6.1 Success:** A debug build (`cargo run`) emits logs in pretty-printed text format with ANSI colors by default
- **helm-chart.AC6.2 Success:** A release build (`cargo build --release && ./target/release/atc-server`) emits logs in single-line JSON format by default, one event per line, each containing `timestamp`, `level`, `fields`, and span context
- **helm-chart.AC6.3 Success:** `ATC_LOG_FORMAT=pretty ./target/release/atc-server` overrides the release-build default to pretty
- **helm-chart.AC6.4 Success:** `ATC_LOG_FORMAT=json cargo run` overrides the debug-build default to JSON

### helm-chart.AC7: Optional templates render correctly
- **helm-chart.AC7.1 Success:** `helm template ... --values tests/values-ingress.yaml` renders a `networking.k8s.io/v1` Ingress with the configured className, hosts, and TLS
- **helm-chart.AC7.2 Success:** `helm template ... --values tests/values-gateway.yaml` renders a `gateway.networking.k8s.io/v1` HTTPRoute with a default rule routing `/` to the Service when `gateway.rules` is empty
- **helm-chart.AC7.3 Success:** `helm template ... --values tests/values-persistence.yaml` renders a PVC with the configured access modes, size, and storage class
- **helm-chart.AC7.4 Success:** `helm template ... --values tests/values-persistence.yaml --set persistence.existingClaim=my-claim` does NOT render a PVC but the Deployment's volume uses `my-claim` as the `persistentVolumeClaim.claimName`
- **helm-chart.AC7.5 Success:** `helm template ... --values tests/values-metrics.yaml` renders a ServiceMonitor targeting the named `metrics` port with the configured interval and scrape timeout
- **helm-chart.AC7.6 Failure:** `helm template ... --set metrics.serviceMonitor.enabled=true --set metrics.enabled=false` does NOT render a ServiceMonitor (both flags required)

### helm-chart.AC8: CI helm job path filtering and matrix
- **helm-chart.AC8.1 Success:** A PR touching only `deploy/helm/atc/values.yaml` runs the `helm` job and skips the `backend` and `frontend` jobs; the `helm-result`, `backend-result`, and `frontend-result` gates all report success
- **helm-chart.AC8.2 Success:** A PR touching only backend code skips the `helm` job; `helm-result` still reports success (skipped-as-passed)
- **helm-chart.AC8.3 Success:** The `helm` job renders and validates all 10 combinations (2 k8s versions × 5 test values files) via `helm template | kubeconform -strict`
- **helm-chart.AC8.4 Failure:** A deliberately broken template (e.g., typo in an API version) fails the `helm` job with a clear kubeconform error message identifying the invalid resource
- **helm-chart.AC8.5 Success:** `zizmor` run against the updated `ci.yml` produces no findings

### helm-chart.AC9: Dockerfile runs as nonroot
- **helm-chart.AC9.1 Success:** `docker run --rm ghcr.io/bojanrajkovic/atc:dev id` reports `uid=65532` and `gid=65532`
- **helm-chart.AC9.2 Success:** `docker run --rm -p 8080:8080 ghcr.io/bojanrajkovic/atc:dev` starts successfully without any `USER` override, binds both listeners, and responds to `/healthz` and `/readyz` on port 8080
- **helm-chart.AC9.3 Success:** The image runs cleanly in a Kubernetes namespace with `pod-security.kubernetes.io/enforce: restricted` without any chart-level `runAsUser` override

### helm-chart.AC10: Chart published via release.yml with attestation
- **helm-chart.AC10.1 Success:** A tag-triggered run of `release.yml` (test fork acceptable) produces an OCI artifact at `oci://ghcr.io/<org>/charts/atc:<version>` after `build-container` succeeds
- **helm-chart.AC10.2 Success:** `helm show chart oci://ghcr.io/<org>/charts/atc --version <version>` returns the expected chart metadata
- **helm-chart.AC10.3 Success:** `helm install atc oci://ghcr.io/<org>/charts/atc --version <version>` succeeds against a real cluster and the installed release passes `helm test atc`
- **helm-chart.AC10.4 Success:** `gh attestation verify` confirms the Sigstore provenance record for the published chart tarball
- **helm-chart.AC10.5 Failure:** A `release.yml` run where `build-container` fails does NOT produce a chart artifact (the `publish-helm-chart` job is gated on `build-container` and `merge-manifest` success)

### helm-chart.AC11: release-please manages chart version and appVersion
- **helm-chart.AC11.1 Success:** A merged `feat(helm): ...` commit causes release-please to open a PR bumping `deploy/helm/atc/Chart.yaml` version (minor) and updating `deploy/helm/atc/CHANGELOG.md`
- **helm-chart.AC11.2 Success:** After release-please opens or updates the release PR, the `sync-helm-app-version` job runs under the releaser-bot identity and rewrites `deploy/helm/atc/Chart.yaml` `appVersion` to match the current linked app version from `.release-please-manifest.json`
- **helm-chart.AC11.3 Success:** An app-only release (no `deploy/helm/**` commits) does NOT bump `Chart.yaml` version but DOES run `sync-helm-app-version` to keep `appVersion` aligned with the latest linked app version
- **helm-chart.AC11.4 Success:** Downstream CI (Backend, Frontend, Helm, PR Checks) runs on release PRs under the bot identity, producing status checks that can satisfy branch protection
- **helm-chart.AC11.5 Success:** The `sync-helm-app-version` job is idempotent — when `appVersion` is already in sync, the `git diff --quiet` guard skips the commit without failing the job

## Glossary

- **atc-server**: The Axum HTTP server crate (`backend/crates/atc-server/`) — the only backend crate that changes in Phase 6.
- **atc-core**: The domain-types crate in the Rust workspace. Unchanged by Phase 6; listed here because reviewers see references to the workspace layout.
- **figment**: A Rust configuration library that layers multiple sources (defaults, files, environment variables). Used here to build the `Config` struct with the `ATC_*` environment-variable prefix and nested `__` key convention.
- **vergen**: A Rust build-script helper that captures compile-time metadata (git SHA, rustc version, build timestamp, target triple) as cargo environment variables, consumed at startup to populate the `build_info` gauge.
- **axum-prometheus**: A middleware crate for Axum that instruments every HTTP request and records request counts and duration histograms into the `metrics` facade.
- **metrics-exporter-prometheus**: The Prometheus backend for the `metrics` facade — installs a global recorder and serves the `/metrics` scrape endpoint.
- **metrics-process**: A `metrics`-compatible collector that periodically samples OS-level process statistics (CPU, memory, file descriptors, start time) and publishes them as Prometheus gauges.
- **kubeconform**: A fast Kubernetes manifest schema validator. Used in CI to check that `helm template` output is valid against upstream Kubernetes JSON schemas and CRD schemas from the datreeio catalog.
- **release-please**: Google's automated release PR bot. Reads conventional commit history to decide version bumps, opens/updates a release PR editing `Chart.yaml`, `Cargo.toml`, `package.json`, and changelogs, then tags and creates a GitHub Release when the PR is merged.
- **helm release-type**: A release-please package type that understands `Chart.yaml` version fields and changelog conventions specific to Helm charts, as opposed to the `rust` or `node` types used for the other packages in this repo.
- **linked-versions plugin**: A release-please plugin that keeps multiple packages (backend + frontend in this repo) on the same version number. The Helm chart deliberately sits outside this group so its version can evolve independently.
- **distroless**: Google's family of minimal OCI base images that contain only a language runtime and CA certificates — no shell, no package manager. `cc-debian13` is the C/C++ variant (correct for Rust binaries). The `:nonroot` tag bakes in UID 65532 so the container starts as a non-root user without a `USER` directive.
- **OCI (registry)**: Open Container Initiative — the standard image and artifact format used by container registries. Helm 3.8+ can push and pull chart tarballs from any OCI registry (`oci://`) in addition to classic HTTP chart repositories.
- **Sigstore / `gh attestation verify`**: A keyless code-signing ecosystem. GitHub Actions can generate a signed provenance record (SLSA build attestation) for any artifact; `gh attestation verify` checks that record against the Sigstore transparency log without requiring you to manage signing keys.
- **Pod Security Standards (PSS) restricted profile**: A Kubernetes admission control policy that enforces a strict set of security constraints on Pods — requires non-root user, read-only root filesystem, no privilege escalation, all Linux capabilities dropped, and a seccomp profile. Namespaces opt in via a label.
- **ServiceMonitor**: A Prometheus Operator CRD that tells Prometheus which Services to scrape and how. The chart's optional `servicemonitor.yaml` template creates one targeting the `metrics` named port.
- **HTTPRoute**: A Gateway API CRD (`gateway.networking.k8s.io/v1`) for configuring HTTP routing rules. The chart provides an optional `httproute.yaml` template as an alternative to the classic `networking.k8s.io/v1` Ingress.
- **Gateway API**: The Kubernetes SIG-Network successor to Ingress — a role-based API with richer routing semantics. Requires a separate gateway controller (e.g., Envoy Gateway, Cilium) to be installed in the cluster.
- **ReadWriteOnce (RWO)**: A Kubernetes PersistentVolume access mode meaning the volume may be mounted by only one Node at a time. Most block-storage StorageClasses are RWO, which makes rolling updates impossible when a PVC is in use — hence the chart's automatic switch to `Recreate` strategy when `persistence.enabled` is true.
- **RollingUpdate vs Recreate**: Kubernetes Deployment update strategies. `RollingUpdate` starts new Pods before terminating old ones (zero downtime); `Recreate` terminates all old Pods before starting new ones (brief downtime, required when only one Pod may hold a ReadWriteOnce volume at a time).
- **values.schema.json**: A JSON Schema file placed at the root of a Helm chart directory. Helm validates the user-supplied `values.yaml` overrides against this schema at `helm install`/`upgrade`/`lint` time, surfacing type errors and missing required fields before any template rendering occurs.
- **`_helpers.tpl`**: The conventional Helm template file for shared named templates (Go template `define` blocks). This chart uses it for label helpers, name helpers, and the ServiceAccount name resolver.
- **NOTES.txt**: A Helm template file rendered and printed to the terminal after a successful `helm install` or `helm upgrade`. Used here to show the operator how to access ATC (port-forward, ingress URL, or gateway URL) depending on which features are enabled.
- **helm test hook**: A Helm feature where a Pod annotated with `helm.sh/hook: test` is run by `helm test <release>` to verify a deployed release is functional. The chart's `test-connection.yaml` Pod hits `/healthz` to confirm the server is reachable.
- **`helm.sh/hook-delete-policy: before-hook-creation,hook-succeeded`**: A Helm annotation that instructs Helm to delete a hook Pod before creating a new one and after a successful run. Failed Pods are retained for log inspection.
- **ingressClassName**: A field on a Kubernetes Ingress resource (and a separate `IngressClass` resource kind) that selects which ingress controller should process the resource. Required in clusters with multiple ingress controllers.
- **lefthook**: A Git hook manager. This project uses it to run linters on pre-commit, validate commit messages on commit-msg, and run tests plus the doc-staleness check on pre-push.
- **mise**: A polyglot tool version manager (successor to `asdf`). All toolchain versions (Rust, Node, `helm`, `kubeconform`, `just`, `lefthook`) are pinned in `.mise.toml` and installed by `mise install`.
- **zizmor**: A static analyzer for GitHub Actions workflow files. Flags common security issues (script injection, excessive permissions, unpinned actions). Run in CI and locally against any workflow file changed in a PR.
- **release-please bot job / app-token pattern**: The pattern used in `release-please.yml` where a GitHub App token is minted at job start, the bot's git identity is resolved via `gh api /app`, and subsequent commits are made under that identity. Allows the bot's commits to trigger downstream CI (personal access tokens do not). The `sync-helm-app-version` job mirrors the existing `refresh-lockfile` job that uses this pattern.
- **dorny/paths-filter**: A GitHub Action that evaluates which path globs were touched by a PR and outputs boolean flags per filter. Used in CI to skip unrelated jobs and still report success for branch protection gates.
- **`{{ fail }}`**: A Helm built-in template function that immediately aborts template rendering with a user-supplied error message. Used here for two cross-field constraints (sqlite URL without a PVC, multi-replica with persistence) that JSON Schema cannot express.

## Architecture

Phase 6 delivers a single publishable Helm chart at `deploy/helm/atc/` and the minimum set of backend changes required for that chart to install cleanly under `restricted` Pod Security Standards on both homelab (k3s) and work clusters.

The chart ships one mandatory workload (a Deployment), one mandatory Service, and a dedicated ServiceAccount. Four optional templates (Ingress, HTTPRoute, PersistentVolumeClaim, ServiceMonitor) are each gated behind independent values flags that default to `false`. A fifth optional artifact, the `helm test` hook Pod, is shipped without a gate — it carries `helm.sh/hook: test` and therefore only runs when an operator explicitly invokes `helm test <release>`, so adding a gate would add a knob with no runtime effect and would complicate the idiomatic `helm test` experience. All resources carry the standard `app.kubernetes.io/*` labels and are namespaced under an `atc.*` template helper set in `_helpers.tpl`. A `values.schema.json` validates operator input at install time.

Three deployment modes are supported, distinguished by the combination of `config.databaseUrl` and `persistence.enabled`:

| Mode | `databaseUrl` | `persistence.enabled` | Strategy | Use case |
|---|---|---|---|---|
| Ephemeral / demo | unset | `false` | `RollingUpdate` (zero downtime) | First-touch homelab, demos |
| Local SQLite | `sqlite:///var/lib/atc/atc.db` | `true` | `Recreate` (brief downtime) | Single-instance homelab or work cluster without external DB |
| External Postgres | `postgres://...` | `false` | `RollingUpdate` (zero downtime) | Work cluster with operated Postgres |

The update strategy is selected automatically based on `persistence.enabled`: when persistence is on, the chart uses `Recreate` because most StorageClasses provide `ReadWriteOnce` volumes that block pod handoff during rolling updates; when persistence is off, the chart uses `RollingUpdate` with `maxSurge: 1, maxUnavailable: 0`. A `{{ fail }}` template guard rejects `sqlite://` URLs pointing outside `/tmp` when `persistence.enabled` is `false`, converting a silent runtime failure into an obvious install-time error.

The backend changes are scoped to `backend/crates/atc-server` and introduce four new capabilities: a figment-based `Config` struct (env var prefix `ATC_*`) replacing hardcoded values, an axum-prometheus metrics layer served on a side-port listener bound to `ATC_METRICS_ADDR` (always bound, regardless of chart-level `metrics.enabled`), JSON-formatted logging in release builds (via `tracing_subscriber::fmt::format::Json` with `cfg!(debug_assertions)` controlling the default), and a renamed routing layer that exposes `/healthz` (liveness) and `/readyz` (readiness) at root — the legacy `/health` endpoint is removed outright since no consumers exist yet. A new `build.rs` runs `vergen` with the gitoxide backend to capture git SHA, rustc version, build timestamp, and target triple at compile time; these feed a registered `atc_build_info` gauge exposed through the metrics layer.

The container runtime image flips from `gcr.io/distroless/cc-debian13` (root) to `gcr.io/distroless/cc-debian13:nonroot` (UID 65532) so the image itself asserts non-root identity and the chart's `restricted` PSS context has nothing to override. One line of Dockerfile change.

The release pipeline gains three integrated mechanisms: `release.yml` gets a `publish-helm-chart` job that runs `helm package` + `helm push` to `oci://ghcr.io/bojanrajkovic/charts/atc` with Sigstore attestation, after container publishing succeeds; `release-please-config.json` gains a fifth package entry for `deploy/helm/atc/` with release-type `helm` (outside the existing `linked-versions` group); and `release-please.yml` gains a second bot job (mirroring the existing `refresh-lockfile` pattern) that mints an app token, reads the current linked app version from `.release-please-manifest.json`, rewrites `Chart.yaml`'s `appVersion` field via `sed`, and commits under the releaser-bot identity. The `if git diff --quiet` guard makes that job idempotent.

CI validation lives in the existing `ci.yml` as a new `helm` job, path-filtered via `dorny/paths-filter` on `deploy/helm/**`. The job runs `helm lint`, then iterates over a matrix of five test values files (defaults + each toggle-on) and two Kubernetes versions (oldest supported + latest stable), rendering each combination with `helm template` and validating against upstream schemas via `kubeconform` with the datreeio CRD catalog for `ServiceMonitor` and `HTTPRoute` validation. A `helm-result` gate job converts skipped runs into passing statuses for branch protection, mirroring the existing `backend-result` and `frontend-result` pattern.

Seven GitHub issues are filed during Phase 6 implementation to track deferred work. One issue — `design: externalize live state to support multi-replica deployments` — is the gating prerequisite for three others (HPA template, PDB template, anti-affinity defaults), all labeled `blocked`. Three issues are independent (NetworkPolicy template, chart-testing on kind, chart-releaser + GitHub Pages). The dependency graph lives in the issues themselves, with the state-externalization issue pointing at a future ADR under `docs/architecture-decisions/` rather than at a design plan.

## Existing Patterns

Codebase investigation (Phase 1 of brainstorming) confirmed this design follows, extends, or deliberately diverges from existing ATC patterns:

**Follows existing patterns:**
- **Cargo workspace layout** — new modules (`config.rs`, `metrics.rs`, `build.rs`) land inside the existing `backend/crates/atc-server/src/` without restructuring. No workspace-level changes.
- **`cfg!(debug_assertions)` for dev/release branching** — the existing asset serving in `assets.rs` already uses this pattern (`dev_proxy` vs `serve_embedded`). The new `LogFormat::default()` reuses it for pretty-vs-JSON logging selection.
- **Three-tier CI layout** — the new `helm` job matches the existing `backend` and `frontend` jobs in `ci.yml`: path-filter detection, main job with mise-provisioned tools, result-gate job for branch protection. No new workflow file.
- **SHA-pinned actions with version comments, `permissions: {}`, `persist-credentials: false`** — every new workflow entry follows the zizmor-clean security posture established in Phase 4.
- **Mise-provisioned tool versions** — new tools (`helm`, `kubeconform`) land in `.mise.toml` rather than being installed by ad-hoc workflow steps. Renovate's mise manager tracks them.
- **App-token bot job for manifest mutations** — the new `sync-helm-app-version` job in `release-please.yml` mirrors the existing `refresh-lockfile` job structure: mint app token, resolve bot identity, checkout release PR branch with persisted credentials, edit one file, commit, push. No new pattern introduced.
- **Doc-staleness gate enforcement** — `scripts/doc-mapping.sh` is updated so every new source file gets mapped to its architecture doc, keeping the pre-push hook honest.
- **`helm.sh/hook-delete-policy: before-hook-creation,hook-succeeded`** — the `helm test` hook Pod follows the standard Helm convention for post-success cleanup with failure retention.
- **`app.kubernetes.io/*` labels with split selector/labels** — selectors use only immutable fields (`name`, `instance`), full labels include version/component/part-of/managed-by. Matches Helm's own recommended pattern and Bitnami's conventions.

**Introduces new patterns (with justification):**
- **figment-based `Config` struct with `ATC_*` env prefix** — atc-server previously had no configuration mechanism beyond hardcoded values and `RUST_LOG`. Figment is chosen over clap or hand-rolled env var reads because its nested env-var convention (`ATC_GITHUB__WEBHOOK_SECRET` → `config.github.webhook_secret`) sets up Phase 8's GitHub config without rework. Documented as a new Decision in `docs/architecture/backend-server.md`.
- **Side-port metrics listener via `tokio::select!` over two `axum::serve` calls** — atc-server previously had a single listener. The side-port pattern is standard across CNCF projects (kube-state-metrics, cert-manager, Linkerd) and documented in the Axum example repo. Metrics port always binds regardless of chart-level `metrics.enabled`; only the Service-level exposure is gated.
- **`tests/` directory convention for chart CI matrix** — new to this repo. Files under `deploy/helm/atc/tests/values-*.yaml` drive the `helm template` + `kubeconform` matrix in CI. The `tests/` naming is more discoverable than alternative conventions (`ci/`, `examples/`) and matches the convention used by `shivjm/helm-kubeconform-action` even though we're hand-rolling the bash.
- **`{{ fail }}` template guards for cross-field validation** — JSON Schema cannot easily express "field A depends on field B," so template-level `fail` calls are the idiomatic Helm answer. The chart uses them for two constraints: `sqlite://` URL on a non-tmp path with `persistence.enabled: false`, and `persistence.enabled: true` with `replicaCount > 1`.

**Deliberately diverges from:**
- **`/health` → `/healthz` rename with no backward-compatible alias** — the existing `backend-server.md` documents `/health` as a root-level infrastructure endpoint. Phase 6 renames it outright (not a deprecation shim) because no consumers exist yet. The Decision in the architecture doc is updated to `/healthz and /readyz stay at root` with the rationale that k8s conventions favor the `-z` suffix for probe endpoints.
- **Dockerfile runtime image** — Phase 5 established `gcr.io/distroless/cc-debian13` as the runtime base (root). Phase 6 flips this to `:nonroot`. The `release-pipeline.md` Decision is updated to record the rationale: asserting non-root identity in the image itself avoids dev/local/k8s drift and makes the restricted-PSS chart context unambiguous.

**No existing patterns found for:**
- **Helm chart publishing workflow** — `release.yml` has no prior chart packaging or OCI publishing steps. The new `publish-helm-chart` job establishes the baseline.
- **`release-please` helm release-type integration** — the existing `release-please-config.json` has rust and node release-types only. The helm release-type is new to this repo.

## Implementation Phases

<!-- START_PHASE_1 -->
### Phase 1: Backend foundations (config, logging, routing rename)

**Goal:** Replace hardcoded configuration and routing in `atc-server` with a figment-based `Config` struct, JSON-structured logging in release builds, a renamed health endpoint, and a new readiness endpoint — all without any metrics or chart work.

**Components:**
- `backend/crates/atc-server/Cargo.toml` — new direct deps: `figment` (with `env` + `toml` features), `serde`/`serde_json` (already present, verify features); enable `json` feature on `tracing-subscriber`
- `backend/crates/atc-server/src/config.rs` (NEW) — `Config` struct with `http_addr`, `metrics_addr`, `database_url`, `log_filter`, `log_format`; `LogFormat` enum (`Pretty`, `Json`) with `cfg!(debug_assertions)`-driven default; `Config::load()` reading from figment defaults + `ATC_*` env prefix
- `backend/crates/atc-server/src/routes.rs` — `/health` handler renamed to `/healthz`; new `/readyz` handler returning trivial ok; `api_routes()` signature unchanged until Phase 2
- `backend/crates/atc-server/src/main.rs` — `Config::load()` replaces hardcoded `0.0.0.0:8080`; tracing_subscriber initialization branches on `cfg.log_format` between `.json()` and `.pretty()`; `EnvFilter::try_new(&cfg.log_filter)` replaces `try_from_default_env`
- `docs/architecture/backend-server.md` — new Configuration section documenting figment, `ATC_*` env prefix, nested `__` convention; new Logging section documenting JSON-by-default-in-release; routing section rewritten for `/healthz` and `/readyz`; `config.rs` added to Files; Last verified date bumped
- `scripts/doc-mapping.sh` — new mapping for `backend/crates/atc-server/src/config.rs` → `backend-server.md`

**Dependencies:** None (first phase; builds on the existing Phase 5 codebase state)

**Done when:**
- `cargo check --locked --workspace` passes with the new deps
- `cargo run` starts atc-server on `0.0.0.0:8080` by default
- `ATC_HTTP_ADDR=127.0.0.1:9999 cargo run` binds to the overridden address
- `curl http://localhost:8080/healthz` returns `{"status":"ok"}`
- `curl http://localhost:8080/readyz` returns `{"status":"ok"}`
- `curl http://localhost:8080/health` returns 404 (no backward-compat alias)
- `cargo build --release` produces a binary that logs in JSON format (`{"timestamp":...,"level":"INFO",...}`)
- `cargo run` (debug) logs in pretty-printed format with ANSI colors
- `cargo fmt --check` and `cargo clippy --all-targets --locked -D warnings` pass
- Covers: `helm-chart.AC1.1`, `helm-chart.AC1.2`, `helm-chart.AC6.1`, `helm-chart.AC6.2`
<!-- END_PHASE_1 -->

<!-- START_PHASE_2 -->
### Phase 2: Backend metrics (side-port, axum-prometheus, build_info, process collector)

**Goal:** Stand up the second listener, wire axum-prometheus middleware into the main router, register the `atc_build_info` gauge from vergen-captured build metadata, and start the process metrics collector.

**Components:**
- `backend/crates/atc-server/Cargo.toml` — add `axum-prometheus`, `metrics`, `metrics-process`
- `backend/crates/atc-server/build.rs` (NEW) — calls `vergen::Emitter::default()` with build, cargo, git (gix backend), and rustc instruction sets; emits `VERGEN_GIT_SHA`, `VERGEN_RUSTC_SEMVER`, `VERGEN_BUILD_TIMESTAMP`, `VERGEN_CARGO_TARGET_TRIPLE` compile-time env vars
- `backend/crates/atc-server/Cargo.toml` (build-dependencies section) — add `vergen` with `build`, `cargo`, `git`, `gix`, `rustc` features
- `backend/crates/atc-server/src/metrics.rs` (NEW) — `build()` function returns `(PrometheusMetricLayer, Router)` where the router exposes `/metrics` on the side-port; internal `register_build_info()` describes and sets the `atc_build_info` gauge with vergen-sourced labels; internal `spawn_process_collector()` starts a 10-second-tick collector task
- `backend/crates/atc-server/src/routes.rs` — `api_routes()` signature takes a `PrometheusMetricLayer`, applies it via `.layer()` to the main router so HTTP request metrics get recorded
- `backend/crates/atc-server/src/main.rs` — calls `metrics::build()` after tracing init; binds both `cfg.http_addr` and `cfg.metrics_addr`; uses `tokio::select!` over two `axum::serve(...).with_graceful_shutdown(shutdown_signal())` calls to serve both listeners with shared Ctrl-C shutdown handling
- `docs/architecture/backend-server.md` — new Metrics section documenting the side-port listener, axum-prometheus layer placement, `atc_build_info` gauge labels, process collector tick interval, and the explicit decision that the listener always binds regardless of chart-level `metrics.enabled`; `metrics.rs` and `build.rs` added to Files
- `scripts/doc-mapping.sh` — new mappings for `backend/crates/atc-server/src/metrics.rs` → `backend-server.md` and `backend/crates/atc-server/build.rs` → `backend-server.md`

**Dependencies:** Phase 1 (Config struct exposes `metrics_addr`; tracing initialized; routes module in the new shape)

**Done when:**
- `cargo check --locked --workspace` passes
- `cargo run` binds `0.0.0.0:8080` and `0.0.0.0:9090` simultaneously; both log their listening addresses at startup
- `curl http://localhost:9090/metrics` returns a Prometheus text-format body containing at minimum: `atc_build_info{version="...",git_sha="...",rustc_version="...",build_timestamp="...",target_triple="..."} 1`, `process_cpu_seconds_total`, `process_resident_memory_bytes`, `process_open_fds`, `axum_http_requests_total`, `axum_http_requests_duration_seconds_bucket`
- Ctrl-C on the running binary shuts both listeners down cleanly without hanging
- `ATC_METRICS_ADDR=127.0.0.1:7777 cargo run` binds the metrics listener to the overridden port
- `cargo fmt --check` and `cargo clippy --all-targets --locked -D warnings` pass
- Covers: `helm-chart.AC2.1`, `helm-chart.AC2.2`, `helm-chart.AC2.3`, `helm-chart.AC2.4`
<!-- END_PHASE_2 -->

<!-- START_PHASE_3 -->
### Phase 3: Helm chart base templates

**Goal:** Create the core chart directory and the mandatory templates (Deployment, Service, ServiceAccount, NOTES.txt) without any optional features. The chart must render and pass `helm lint` in isolation.

**Components:**
- `deploy/helm/atc/Chart.yaml` — `name: atc`, `type: application`, `version: 0.1.0`, `appVersion: "0.2.0"` (matching the current linked app version), `kubeVersion: ">=1.29.0-0"`, maintainers, description
- `deploy/helm/atc/values.yaml` — full values surface per Section 2 of brainstorming, with extensive comments on each block including the three-mode storage matrix and the `replicaCount` multi-replica rationale
- `deploy/helm/atc/values.schema.json` — JSON Schema draft 2020-12 covering every field in `values.yaml` with types, enums for discrete choices, and descriptions
- `deploy/helm/atc/README.md` — rendered config table, install instructions (OCI + local source), the three supported modes with copy-paste snippets
- `deploy/helm/atc/LICENSE` — copy of repo Apache-2.0
- `deploy/helm/atc/.helmignore` — excludes `tests/` (CI fixtures, not shipped), `README.md.gotmpl`, etc.
- `deploy/helm/atc/templates/_helpers.tpl` — `atc.name`, `atc.fullname`, `atc.chart`, `atc.labels`, `atc.selectorLabels`, `atc.serviceAccountName` per Section 3 of brainstorming
- `deploy/helm/atc/templates/NOTES.txt` — post-install guidance with conditional blocks for ingress/gateway/port-forward paths and the `existingSecret.name` warning
- `deploy/helm/atc/templates/deployment.yaml` — single mandatory workload with conditional `strategy` block based on `persistence.enabled`, restricted pod and container securityContext defaults (overridable via values for legitimate operator edge cases including storage CSI drivers with UID constraints, sidecars needing writable rootfs, profilers needing elevated capabilities), UID 65532, readOnlyRootFilesystem, capabilities dropped, seccompProfile: RuntimeDefault, probes pointing at `/healthz` + `/readyz`, env var wiring for `ATC_*` variables and `existingSecret` `secretKeyRef` blocks, the `{{ fail }}` guards for the sqlite-without-persistence and persistence-with-multi-replica foot-guns
- `deploy/helm/atc/templates/service.yaml` — ClusterIP with `http` named port unconditionally and `metrics` named port gated on `metrics.enabled`
- `deploy/helm/atc/templates/serviceaccount.yaml` — gated on `serviceAccount.create`, `automountServiceAccountToken: false`
- `docs/architecture/deployment.md` (NEW) — Purpose, Key Decisions (with rejected alternatives for restricted-PSS-by-default, OCI-only publishing, three storage modes, conditional update strategy, split-port metrics), Boundaries, Files
- `scripts/doc-mapping.sh` — new glob mapping `deploy/helm/**` → `deployment.md`
- `CLAUDE.md` — Project Structure updated to reference `deploy/helm/`; Documentation Map updated to reference `docs/architecture/deployment.md`; Last verified bumped

**Dependencies:** Phase 2 (chart's probe paths and env var shapes depend on the backend actually having `/healthz`, `/readyz`, `ATC_HTTP_ADDR`, `ATC_METRICS_ADDR`)

**Done when:**
- `helm lint deploy/helm/atc` passes with no errors or warnings
- `helm template atc deploy/helm/atc` renders successfully with no optional features enabled
- The rendered Deployment passes restricted Pod Security Standards when fed to `kubectl apply --dry-run=server` against a cluster with PSS admission
- `helm template atc deploy/helm/atc --set config.databaseUrl=sqlite:///var/lib/atc/atc.db --set persistence.enabled=false` fails with the `{{ fail }}` guard's message
- `helm template atc deploy/helm/atc --set persistence.enabled=true --set replicaCount=3` fails with the multi-replica `{{ fail }}` guard's message
- Default render uses `strategy: RollingUpdate`; `--set persistence.enabled=true` flips it to `strategy: Recreate`
- Covers: `helm-chart.AC3.1`, `helm-chart.AC3.2`, `helm-chart.AC3.3`, `helm-chart.AC3.4`, `helm-chart.AC3.5`, `helm-chart.AC4.1`, `helm-chart.AC5.1`, `helm-chart.AC5.2`, `helm-chart.AC5.3`
<!-- END_PHASE_3 -->

<!-- START_PHASE_4 -->
### Phase 4: Helm chart optional templates and CI values matrix

**Goal:** Add every optional template (Ingress, HTTPRoute, PVC, ServiceMonitor, `helm test` hook) and create the `tests/values-*.yaml` matrix that CI will render against.

**Components:**
- `deploy/helm/atc/templates/ingress.yaml` — gated on `ingress.enabled`, standard `networking.k8s.io/v1` shape with operator-supplied `ingressClassName`, hosts, paths, TLS
- `deploy/helm/atc/templates/httproute.yaml` — gated on `gateway.enabled`, `gateway.networking.k8s.io/v1` HTTPRoute with operator-supplied `parentRefs`, optional `hostnames`, and a default rule routing `/` to the Service when `gateway.rules` is empty
- `deploy/helm/atc/templates/pvc.yaml` — gated on `persistence.enabled AND NOT persistence.existingClaim`; handles the Bitnami `storageClass: "-"` convention for opting out of default StorageClasses
- `deploy/helm/atc/templates/servicemonitor.yaml` — gated on both `metrics.enabled AND metrics.serviceMonitor.enabled`; `monitoring.coreos.com/v1` CRD; targets the named `metrics` port
- `deploy/helm/atc/templates/tests/test-connection.yaml` — `helm test` hook Pod using pinned `busybox:1.37.0`, running with restricted-PSS security context, `wget`-ing `http://<fullname>:<port>/healthz`
- `deploy/helm/atc/tests/values-defaults.yaml` — empty overrides (everything off)
- `deploy/helm/atc/tests/values-ingress.yaml` — `ingress.enabled: true` with `className: nginx`, one host, TLS secret reference
- `deploy/helm/atc/tests/values-gateway.yaml` — `gateway.enabled: true` with a sample `parentRefs` entry
- `deploy/helm/atc/tests/values-persistence.yaml` — `persistence.enabled: true` plus `config.databaseUrl: sqlite:///var/lib/atc/atc.db`
- `deploy/helm/atc/tests/values-metrics.yaml` — `metrics.enabled: true` and `metrics.serviceMonitor.enabled: true`
- `deploy/helm/atc/values.schema.json` — updated to cover the new fields exposed by these templates

**Dependencies:** Phase 3 (base chart must exist first)

**Done when:**
- `helm lint deploy/helm/atc` still passes
- `helm template atc deploy/helm/atc --values deploy/helm/atc/tests/<each-file>.yaml` renders successfully for all five matrix files
- Rendered `servicemonitor.yaml` only appears when both `metrics.enabled` and `metrics.serviceMonitor.enabled` are true
- Rendered `pvc.yaml` does NOT appear when `persistence.existingClaim` is set; Deployment's volume entry uses the existing claim name instead
- `helm template atc deploy/helm/atc --values .../values-gateway.yaml` produces an HTTPRoute with a default rule routing `/` to the Service
- Covers: `helm-chart.AC4.2`, `helm-chart.AC4.3`, `helm-chart.AC4.4`, `helm-chart.AC4.5`, `helm-chart.AC5.4`, `helm-chart.AC5.5`, `helm-chart.AC7.1`, `helm-chart.AC7.2`
<!-- END_PHASE_4 -->

<!-- START_PHASE_5 -->
### Phase 5: CI helm job + justfile recipes

**Goal:** Wire the chart into the existing CI pipeline so every PR touching `deploy/helm/**` runs lint + render matrix + schema validation against two Kubernetes versions, and add local developer ergonomics via justfile recipes that mirror CI.

**Components:**
- `.mise.toml` — add `helm = "3.16.2"` and `kubeconform = "0.6.7"` (or latest stable at implementation time); Renovate's mise manager will track future bumps
- `.github/workflows/ci.yml` — add `helm: 'deploy/helm/**'` to the `dorny/paths-filter` filters; add a new `helm` job that installs helm + kubeconform via mise-action, runs `helm lint`, then iterates a 2x5 matrix (k8s versions × test values files) calling `helm template | kubeconform` with the datreeio CRD catalog schema location for ServiceMonitor and HTTPRoute; add a `helm-result` gate job mirroring `backend-result`/`frontend-result`
- `justfile` — new recipes: `helm-lint`, `helm-template` (loops over test values files, no validation), `helm-check` (loops over the same k8s-version × values matrix as CI and calls kubeconform), `helm-package` (packages to `./dist/`); top-level `lint`, `check`, and `test` recipes updated to fan out to helm commands in parallel with existing backend/frontend commands
- `docs/architecture/ci-pipeline.md` — new section documenting the `helm` job with its path filter, matrix dimensions, kubeconform schema source, and `helm-result` gate rationale
- `scripts/doc-mapping.sh` — new mapping for `.github/workflows/ci.yml` (helm job) if not already covered

**Dependencies:** Phase 4 (chart + test values files must exist for the CI matrix to have anything to validate)

**Done when:**
- `just helm-lint` passes locally
- `just helm-check` passes locally against both configured k8s versions and all five values files
- A PR touching only `deploy/helm/atc/values.yaml` triggers the new `helm` job in CI and skips `backend`/`frontend`
- A PR touching only backend code skips the `helm` job and the `helm-result` gate still reports success for branch protection
- The CI `helm` job catches a deliberately-broken template (e.g., typo in an API version) with a clear kubeconform error message
- `zizmor` run against the updated `ci.yml` produces no findings
- Covers: `helm-chart.AC8.1`, `helm-chart.AC8.2`, `helm-chart.AC8.3`, `helm-chart.AC8.4`
<!-- END_PHASE_5 -->

<!-- START_PHASE_6 -->
### Phase 6: Dockerfile :nonroot flip + release.yml chart publishing

> **Revised 2026-04-08:** The Dockerfile flip was already completed during Phase 5 work; Phase 6 verifies rather than modifies the runtime base image. See docs/architecture/release-pipeline.md for the decision record.

**Goal:** Verify the container runtime base is already `:nonroot` (landed in Phase 5) and wire chart packaging + OCI publishing into the tag-triggered release workflow with Sigstore attestation.

**Components:**
- `Dockerfile` — verify the existing final `FROM` stage is already `gcr.io/distroless/cc-debian13:nonroot` with `USER 65532:65532` (landed in Phase 5, revised here for alignment); add `EXPOSE 9090` for the metrics side port
- `.github/workflows/release.yml` — new `publish-helm-chart` job gated on `needs: [create-release, build-container, merge-manifest]`; installs helm via mise-action; reads the chart version via `helm show chart | yq '.version'`; runs `helm package deploy/helm/atc --destination ./dist`; authenticates to ghcr.io via `GITHUB_TOKEN`; runs `helm push ./dist/atc-${VERSION}.tgz oci://ghcr.io/bojanrajkovic/charts`; runs `actions/attest-build-provenance` on the packaged chart; workflow permissions updated (`contents: read`, `packages: write`, `id-token: write` on that job); `persist-credentials: false` on checkout
- `docs/architecture/release-pipeline.md` — two new Decision entries: "Runtime base is `gcr.io/distroless/cc-debian13:nonroot` (UID 65532)" with rationale about dev/prod identity consistency; "Helm chart published to `oci://ghcr.io/bojanrajkovic/charts/atc` via tag-triggered `release.yml` with Sigstore attestation" with rationale about matching the existing binary/container provenance pattern; Last verified date bumped
- `scripts/doc-mapping.sh` — ensure `Dockerfile` is mapped to `release-pipeline.md` (may have been missing since Phase 5)

**Dependencies:** Phase 5 (chart must pass CI validation before we start publishing it)

**Done when:**
- `docker run --rm ghcr.io/bojanrajkovic/atc:dev id` reports `uid=65532` (not root) *(Dockerfile :nonroot flip already merged in Phase 5; this phase verifies the image still satisfies AC9 after Phase 1–2 backend changes land)*
- `docker run --rm ghcr.io/bojanrajkovic/atc:dev` starts, binds both listeners, and responds to `/healthz` and `/readyz` without any PodSecurityContext overrides
- A tag-triggered run of `release.yml` in a test fork produces an OCI chart artifact at `oci://ghcr.io/<test-fork>/charts/atc:<version>`
- The Sigstore attestation for the chart is verifiable via `gh attestation verify`
- `zizmor` run against the updated `release.yml` produces no findings
- Covers: `helm-chart.AC9.1`, `helm-chart.AC9.2`, `helm-chart.AC9.3`, `helm-chart.AC10.1`, `helm-chart.AC10.2`
<!-- END_PHASE_6 -->

<!-- START_PHASE_7 -->
### Phase 7: release-please integration (5th package + appVersion sync bot job)

**Goal:** Make the Helm chart a first-class release-please package so its version bumps from conventional commits, and add a bot job that keeps `Chart.yaml`'s `appVersion` in lockstep with the linked app version.

**Components:**
- `release-please-config.json` — add `deploy/helm/atc` as a 5th entry in `packages` with `release-type: helm`; leave it OUT of the `linked-versions` plugin (chart version is independent)
- `.release-please-manifest.json` — add `"deploy/helm/atc": "0.1.0"` entry to bootstrap the chart version
- `.github/workflows/release-please.yml` — new job `sync-helm-app-version` gated on `needs: [release-please, refresh-lockfile]` and `if: needs.release-please.outputs.pr != ''`; mints an app token via `actions/create-github-app-token` with `RELEASER_BOT_CLIENT_ID` + `RELEASER_BOT_PRIVATE_KEY`; resolves the bot identity via `gh api /app`; checks out the release PR branch with persisted bot credentials; reads the current `backend/crates/atc-server` version from `.release-please-manifest.json` via `jq`; rewrites `deploy/helm/atc/Chart.yaml`'s `appVersion` via `sed`; guards with `git diff --quiet` for idempotency; commits and pushes under the bot identity with message `chore(helm): sync appVersion to <version>`
- `docs/architecture/release-pipeline.md` — new Decision entry documenting the helm release-type choice with rejected alternatives (why chart is NOT in `linked-versions`, why `extra-files` JSONPath was rejected for appVersion sync, why post-release-please bot job mirrors the lockfile-refresh pattern); Last verified bumped

**Dependencies:** Phase 6 (chart must be publishable before we start auto-bumping its version)

**Done when:**
- Local `release-please --dry-run` against the updated config produces a release PR that updates `deploy/helm/atc/Chart.yaml` version correctly when a `feat(helm):` commit is present
- A commit matching `feat(helm): ...` lands on `main` and the release-please run opens a PR bumping `Chart.yaml` version
- The `sync-helm-app-version` job runs on that PR under the bot identity and updates `Chart.yaml` `appVersion` to match the current linked app version
- Downstream CI (Backend, Frontend, Helm, PR Checks) runs on the release PR under the bot identity, producing the expected status checks for branch protection
- An app-only release (no commits touching `deploy/helm/**`) still triggers the `sync-helm-app-version` job and updates `appVersion` if needed, without bumping the chart `version` field
- `zizmor` run against the updated `release-please.yml` produces no findings
- Covers: `helm-chart.AC11.1`, `helm-chart.AC11.2`, `helm-chart.AC11.3`, `helm-chart.AC11.4`
<!-- END_PHASE_7 -->

<!-- START_PHASE_8 -->
### Phase 8: Deferred-work tracking + doc housekeeping

**Goal:** File the GitHub issues that track work deferred from Phase 6, create the GitHub labels that categorize them, and clean up any remaining documentation loose ends. This is the last Phase 6 task before the phase ships.

**Components:**
- Create GitHub labels (idempotent, skip if exists):
  - `deployment` (color: `0E8A16`) — "Helm chart, k8s manifests, container, release pipeline"
  - `design` (color: `5319E7`) — "Requires design work (ADR or design plan) before implementation"
  - `blocked` (color: `B60205`) — "Gated on another issue or external prerequisite"
  - `enhancement` — assumed to exist (GitHub default); skip
- File seven GitHub issues in `bojanrajkovic/atc` with full bodies referencing `docs/design-plans/2026-04-08-helm-chart.md`:
  1. `design: externalize live state to support multi-replica deployments` — labels `design`, `deployment`. Body: problem statement per `docs/ideation/architecture-research.md`, three architectural options, points at an ADR path under `docs/architecture-decisions/`
  2. `feat(helm): add HorizontalPodAutoscaler template` — labels `enhancement`, `deployment`, `blocked`. Body: "Blocked by #<issue-1>"
  3. `feat(helm): add PodDisruptionBudget template` — labels `enhancement`, `deployment`, `blocked`. Body: "Blocked by #<issue-1>"
  4. `feat(helm): add anti-affinity defaults for multi-replica` — labels `enhancement`, `deployment`, `blocked`. Body: "Blocked by #<issue-1>"
  5. `feat(helm): add NetworkPolicy template` — labels `enhancement`, `deployment`. Not blocked; deferred scope reason
  6. `feat(ci): add chart-testing (ct install) on ephemeral kind cluster` — labels `enhancement`, `deployment`. Not blocked
  7. `feat(release): publish chart via chart-releaser to GitHub Pages` — labels `enhancement`, `deployment`. Not blocked; distribution-channel addition
- `~/Working/projects/atc/BOOTSTRAP.md` — Phase 6 entry gets its full implementation-notes bullet list (mirroring Phases 1–5), referencing the design plan, implementation plan, and test plan paths
- `~/Working/projects/atc/INDEX.yaml` (if present) — update project metadata with Phase 6 completion date

**Dependencies:** Phase 7 (all code and docs must be landed before we file the issues that reference them)

**Done when:**
- All seven issues exist in `bojanrajkovic/atc` with correct labels and bodies
- Issue #1 is referenced by issues 2, 3, and 4 via "Blocked by" body text
- `gh issue list --label deployment` returns all seven
- `gh issue list --label blocked` returns issues 2, 3, and 4
- BOOTSTRAP.md Phase 6 entry has the full implementation-notes block
- A final pre-push hook run on the Phase 6 branch reports no doc staleness
- No AC coverage (this phase is operational, not functional)
<!-- END_PHASE_8 -->

## Additional Considerations

**Error handling:** The chart uses `{{ fail }}` template guards for two cross-field constraints that JSON Schema cannot express — both convert silent runtime failures into obvious install-time errors with clear, actionable messages. At the backend level, `Config::load()` errors propagate via `?` to `main` and exit the process non-zero so Kubernetes sees `CrashLoopBackOff` with a clear log line rather than a hung pod. Metrics listener bind failures are treated the same way as main listener bind failures — neither is silently tolerated.

**Edge cases:**
- **Chart-only releases:** A `feat(helm): ...` commit with no touches to `backend/` or `frontend/` bumps `Chart.yaml.version` but leaves `appVersion` alone — the `sync-helm-app-version` bot job's `git diff --quiet` guard makes this a no-op commit.
- **App-only releases:** A `feat(server): ...` commit with no touches to `deploy/helm/**` leaves `Chart.yaml.version` unchanged but the `sync-helm-app-version` job still runs and updates `appVersion` to the new linked version.
- **Empty Prometheus scrape with metrics disabled at the chart level:** The backend listener still binds `0.0.0.0:9090` inside the pod — the chart just doesn't expose that port on the Service or template a ServiceMonitor. Nothing scrapes it; it costs nothing.
- **`helm test` in a restricted namespace:** The test Pod itself uses the same restricted PSS context as the main workload (non-root, read-only rootfs, dropped caps, seccompProfile RuntimeDefault) so it runs in namespaces with Pod Security admission enforcing `restricted`.

**Future extensibility:** The figment-based `Config` struct uses `Env::prefixed("ATC_")` with the nested `__` convention (`ATC_GITHUB__WEBHOOK_SECRET` → `config.github.webhook_secret`), which means Phases 7–9 can extend it with nested sub-structs (`GithubConfig`, `DatabaseConfig`) without touching the loading path. The same applies to the `metrics` module: new business metrics go through the `metrics` facade (`metrics::counter!`, `metrics::histogram!`) and surface automatically on the existing `/metrics` endpoint — no second exporter needed.

**Documents to Update (design-to-docs contract):**

| Document | Change | Phase |
|---|---|---|
| `docs/architecture/backend-server.md` | Rewrite routing section; new Configuration, Metrics, Logging sections; Files list updated; Last verified bumped | Phases 1–2 |
| `docs/architecture/release-pipeline.md` | Three new Decisions: OCI chart publishing, Dockerfile `:nonroot`, release-please helm release-type + appVersion sync pattern; Last verified bumped | Phases 6–7 |
| `docs/architecture/ci-pipeline.md` | New helm job documentation: path filter, matrix dimensions, schema location, `helm-result` gate | Phase 5 |
| `docs/architecture/deployment.md` (NEW) | Create from scratch: Purpose, Key Decisions (restricted PSS, OCI-only publishing, dual Ingress + HTTPRoute, three storage modes, conditional update strategy, split-port metrics), Boundaries, Files | Phase 3 |
| `scripts/doc-mapping.sh` | Add: `config.rs`, `metrics.rs`, `build.rs` → `backend-server.md`; `deploy/helm/**` → `deployment.md`; `Dockerfile` → `release-pipeline.md`; `release-please.yml` sync job coverage | Phases 1, 2, 3, 6 |
| `CLAUDE.md` | Add `deploy/helm/` and `Dockerfile` to Project Structure; add `deployment.md` to Documentation Map; Last verified bumped | Phase 3 |
| `~/Working/projects/atc/BOOTSTRAP.md` | Phase 6 entry gets implementation-notes block mirroring Phases 1–5; Phase 7 and Phase 9 forward-looking notes + Cross-phase technical debt section already landed in design brainstorming | Phase 8 |

**Implementation scoping:** This design has exactly 8 phases, matching the writing-implementation-plans limit. No scoping or splitting required.
