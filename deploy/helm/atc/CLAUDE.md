# CLAUDE.md — deploy/helm/atc

Last verified: 2026-05-04

> Canonical documentation lives in `docs/architecture/deployment.md`. This file provides domain-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Helm chart packaging ATC for Kubernetes deployment. Published as OCI artifact to `oci://ghcr.io/bojanrajkovic/charts/atc` via the tag-triggered release workflow.

## Key Files

| File | Role |
|------|------|
| `Chart.yaml` | Chart metadata and version |
| `values.yaml` | Default values (restricted Pod Security Standards) |
| `values.schema.json` | JSON Schema for values validation |
| `templates/` | Kubernetes manifests (Deployment, Service, ServiceAccount, optional Ingress/HTTPRoute/PVC/ServiceMonitor) |
| `tests/` | helm-unittest test suites |

## Contracts

- **Restricted security by default:** `runAsNonRoot: true`, UID 65532, seccomp RuntimeDefault. Overridable via values for operator edge cases.
- **Optional resources gated by flags:** Ingress, HTTPRoute, PVC, ServiceMonitor each default to `false`.
- **PgBouncer + listener compatibility:** Operators running the main pool through transaction-mode PgBouncer MUST set `ATC_DATABASE_LISTENER_URL` (via `config.databaseListenerUrl` or `existingSecret.databaseListenerUrlKey`) to point the PG listener at a session-mode endpoint. Transaction-mode PgBouncer reassigns the underlying connection between transactions, silently dropping `LISTEN` registrations and breaking the listener task. The `existingSecret` path wins over the plain-value path when both are set.

## Commands

```bash
helm lint deploy/helm/atc                      # Lint chart
helm template atc deploy/helm/atc              # Render templates
helm unittest deploy/helm/atc                  # Run helm-unittest suites
helm template atc deploy/helm/atc | kubeconform -strict  # Validate against k8s schemas
```

## Key References

- Architecture: `docs/architecture/deployment.md`
- CI matrix: `docs/architecture/ci-pipeline.md` (Helm job section)
