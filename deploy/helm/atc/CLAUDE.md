# CLAUDE.md — deploy/helm/atc

Last verified: 2026-05-10

> Canonical documentation lives in `docs/architecture/deployment.md`. This file provides domain-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Helm chart packaging ATC for Kubernetes deployment. Published via two parallel channels on tag-triggered release: OCI on `oci://ghcr.io/bojanrajkovic/charts/atc` (Sigstore-attested) and a classic HTTP Helm repo at `https://bojanrajkovic.github.io/atc/charts` (no auth required). Index path uses the `/charts` subpath so the gh-pages root stays available for a future docs site — see `docs/architecture/release-pipeline.md` for the URL-stability rationale and `deploy/helm/cr.yaml` for the chart-releaser config.

## Key Files

| File | Role |
|------|------|
| `Chart.yaml` | Chart metadata and version |
| `values.yaml` | Default values (restricted Pod Security Standards) |
| `values.schema.json` | JSON Schema for values validation (`additionalProperties: false`) |
| `templates/` | Kubernetes manifests (Deployment, Service, ServiceAccount, optional Ingress/HTTPRoute/ServiceMonitor) |
| `tests/` | helm-unittest test suites |
| `ci/test-values.yaml` | `ct install` values fixture consumed by the `helm-install` CI job (image override + `pullPolicy: Never` for kind-loaded image) |

## Contracts

- **Restricted security by default:** `runAsNonRoot: true`, UID 65532, seccomp RuntimeDefault. Overridable via values for operator edge cases.
- **Optional resources gated by flags:** Ingress, HTTPRoute, ServiceMonitor each default to `false`.
- **PgBouncer + listener compatibility:** Operators running the main pool through transaction-mode PgBouncer MUST set `ATC_DATABASE_LISTENER_URL` (via `config.databaseListenerUrl` or `existingSecret.databaseListenerUrlKey`) to point the PG listener at a session-mode endpoint. Transaction-mode PgBouncer reassigns the underlying connection between transactions, silently dropping `LISTEN` registrations and breaking the listener task. The `existingSecret` path wins over the plain-value path when both are set.
- **Multi-replica precondition:** `replicaCount > 1` requires a PostgreSQL connection string via either `config.databaseUrl` or `existingSecret.name`+`existingSecret.databaseUrlKey`. Enforced at template-render time via a `{{ fail }}` guard in `templates/deployment.yaml`.
- **URL scheme validation:** the inline `config.databaseUrl` path is rejected at render time unless it starts with `postgres://` or `postgresql://`. The `existingSecret` path is opaque at render time and falls through to a startup-time scheme check in the binary (`ensure_pg_scheme()` in `backend/crates/atc-server/src/main.rs`), which exits with a remediation-naming log line before any sqlx connect call.
- **Sticky sessions are NOT required.** Reconnect-then-snapshot via `/v1/state`+`lastSeq` is the design. Configuring sticky cookies is discouraged outside specific cost-tuning scenarios — it can mask gap-healing regressions in development.
- **Pod anti-affinity** ships on by default via `podAntiAffinity.type` (`soft` / `hard` / `off`). A non-empty `affinity:` value fully overrides the chart's injection. Canonical write-up: `docs/architecture/deployment.md` § Multi-replica.
- **PDB / HPA defaults are not provided.** Tracked as #9 / #8.
- **Graceful shutdown surface:** `shutdown.preStopSleepSeconds` (default 5; `0` opts out and omits the `lifecycle` block) and `shutdown.terminationGracePeriodSeconds` (default 30) pair the EndpointSlice/preStop drain with `atc-server`'s ~13 s in-process budget. The `preStop` hook uses Kubernetes' native `Sleep` action (KEP-3960) — required because the runtime image is Distroless `cc:nonroot` (no `sleep` binary). `kubeVersion` is pinned to `>=1.32.0-0`. Canonical write-up: `docs/architecture/deployment.md` § Graceful shutdown.

## Commands

```bash
helm lint deploy/helm/atc                      # Lint chart
helm template atc deploy/helm/atc              # Render templates
helm unittest deploy/helm/atc                  # Run helm-unittest suites
helm template atc deploy/helm/atc | kubeconform -strict  # Validate against k8s schemas
```

## Key References

- Architecture: `docs/architecture/deployment.md`
- Multi-replica smoke test runbook: `docs/architecture/deployment.md#multi-replica-smoke-test`
- CI matrix: `docs/architecture/ci-pipeline.md` (Helm job section — covers `helm-lint`, `helm` kubeconform matrix, and `helm-install` kind+chart-testing)
