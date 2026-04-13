# CLAUDE.md — deploy/helm/atc

Last verified: 2026-04-12

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
