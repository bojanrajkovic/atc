# Deployment — Architecture

Last verified: 2026-05-04

## Purpose

The ATC Helm chart (`deploy/helm/atc/`) packages the ATC server for deployment to any
conformant Kubernetes cluster. It produces a single mandatory Deployment backed by a
ClusterIP Service and a dedicated ServiceAccount. Optional resources (Ingress, HTTPRoute,
PersistentVolumeClaim, ServiceMonitor, `helm test` hook) are each gated behind independent
values flags that default to `false`.

The chart is published as an OCI artifact to `oci://ghcr.io/bojanrajkovic/charts/atc` via
the tag-triggered release workflow, alongside the container image and binary artifacts.

## Key Decisions

**Decision:** Restricted Pod Security Standards by default, overridable via values for legitimate operator edge cases
**Alternatives considered:** Hardcoded immutable security context; permissive defaults with a "hardened" values preset; let operators opt in to restricted contexts
**Rationale:** The chart ships with restricted Pod Security Standards as the default values in `values.yaml` (`podSecurityContext` and `securityContext`). These defaults satisfy AC5.1/AC5.2/AC5.3 security controls and match what the distroless `:nonroot` image already guarantees at UID 65532. The fields are exposed in `values.yaml` and `values.schema.json`, allowing operators to override them via `--set` for legitimate edge cases: storage CSI drivers with non-standard UID constraints, sidecars requiring writable root filesystems, profilers needing elevated capabilities, etc. Organizations wanting to enforce the restricted profile cluster-wide should use ValidatingAdmissionPolicy or Kyverno at the cluster level, not chart-level immutability. This approach balances secure-by-default with operational flexibility.

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

**Decision:** Three storage modes (ephemeral, local SQLite, external Postgres) with `{{ fail }}` guards for cross-field misconfigurations
**Alternatives considered:** Single mode with external database required; separate chart variants per mode
**Rationale:** ATC's primary early audience is homelab operators who want zero external dependencies. Supporting SQLite with a PVC covers that case cleanly. External Postgres coverage is provided without separate chart variants by parameterizing the connection string. Cross-field constraints (sqlite URL without a PVC; multi-replica with a ReadWriteOnce volume) are enforced at render time via `{{ fail }}` because JSON Schema cannot express conditional dependencies between fields.

**Decision:** Deployment update strategy selected automatically based on `persistence.enabled`
**Alternatives considered:** Always Recreate; let operator choose via explicit `strategy` value; document-only approach with no enforcement
**Rationale:** ReadWriteOnce volumes make rolling updates fail silently when a new Pod is scheduled on a different node — the new Pod hangs pending because the volume is already attached elsewhere. Automatic `Recreate` when `persistence.enabled=true` removes a common foot-gun. Operators who need zero-downtime with persistence must use a ReadWriteMany storage class or an external database (Mode 3), which is documented in values.yaml.

**Decision:** Metrics port always bound in the container; `metrics.enabled` gates only Service-level exposure
**Alternatives considered:** Conditional metrics listener based on chart flag; separate metrics Deployment
**Rationale:** The metrics listener is a backend concern — the binary always binds both ports. Gating the Service port exposure keeps Prometheus scraping optional without requiring chart-level changes to the container runtime behavior. This matches how CNCF projects (cert-manager, Linkerd) handle the same pattern.

**Decision:** OCI-only chart publishing for Phase 3 (`oci://ghcr.io/bojanrajkovic/charts/atc`)
**Alternatives considered:** chart-releaser + GitHub Pages HTTP repository; both OCI and Pages
**Rationale:** OCI eliminates the need for a separate `gh-pages` branch, a chart index, and a GitHub Pages site. Helm 3.8+ supports OCI natively. GitHub Pages + chart-releaser is deferred as a future-work issue for operators who cannot use OCI registries.

**Decision:** Dual routing support — optional Ingress (`networking.k8s.io/v1`) and optional HTTPRoute (`gateway.networking.k8s.io/v1`)
**Alternatives considered:** Ingress only; Gateway API only; neither (document port-forward)
**Rationale:** Ingress covers clusters with a classic ingress controller (nginx, traefik). HTTPRoute covers clusters running a Gateway API controller (Envoy Gateway, Cilium). Providing both optional templates at the same chart version avoids forking. The default is neither — port-forward instructions in NOTES.txt cover the zero-dependency case. (Optional templates added in Phase 4.)

## Environment Variables (ATC_* surface)

The chart wires Helm values to ATC_* environment variables. The canonical list is in `backend/crates/atc-server/src/config.rs`. Phase 2d adds one new optional variable:

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

## Boundaries

**Owns:** Kubernetes resource templates (Deployment, Service, ServiceAccount, and optional resources), values schema validation, post-install operator guidance (NOTES.txt), chart packaging and OCI publishing
**Does not own:** Container image build (Dockerfile, release.yml), backend configuration (ATC_* env vars are the interface), Kubernetes cluster provisioning, Ingress controller or Gateway controller installation
**Prohibitions:** Do not embed secrets in chart templates — operators must use `existingSecret` or provide values at install time. Do not add optional templates to this phase — Ingress, HTTPRoute, PVC, ServiceMonitor, and the test hook are Phase 4.

## Files

- `deploy/helm/atc/Chart.yaml` — Chart identity (name, version, appVersion, kubeVersion, maintainers)
- `deploy/helm/atc/values.yaml` — Full values surface with inline documentation of the three storage modes
- `deploy/helm/atc/values.schema.json` — JSON Schema draft 2020-12 for all values fields; type and enum validation at install/upgrade/lint time
- `deploy/helm/atc/LICENSE` — Apache-2.0 license (copy of repo root LICENSE)
- `deploy/helm/atc/.helmignore` — Excludes CI test fixtures and helm-docs source template from the chart tarball
- `deploy/helm/atc/templates/_helpers.tpl` — Named template helpers: `atc.name`, `atc.fullname`, `atc.chart`, `atc.labels`, `atc.selectorLabels`, `atc.serviceAccountName`
- `deploy/helm/atc/templates/deployment.yaml` — Mandatory workload with conditional update strategy, restricted PSS security contexts, `{{ fail }}` guards, env var wiring, and emptyDir/PVC volume mounts
- `deploy/helm/atc/templates/service.yaml` — ClusterIP Service with `http` port always present and `metrics` port gated on `metrics.enabled`
- `deploy/helm/atc/templates/serviceaccount.yaml` — ServiceAccount gated on `serviceAccount.create`; `automountServiceAccountToken: false`
- `deploy/helm/atc/templates/NOTES.txt` — Post-install guidance with conditional ingress/gateway/port-forward branches and plain-credentials warning
- `deploy/helm/atc/templates/ingress.yaml` — Optional Ingress (`networking.k8s.io/v1`), gated on `ingress.enabled`; supports TLS, hosts, and custom annotations
- `deploy/helm/atc/templates/httproute.yaml` — Optional HTTPRoute (`gateway.networking.k8s.io/v1`), gated on `gateway.enabled`; validates non-empty `parentRefs` via `{{ fail }}` guard
- `deploy/helm/atc/templates/pvc.yaml` — Optional PersistentVolumeClaim, gated on `persistence.enabled && !persistence.existingClaim`; supports custom storage class and access modes
- `deploy/helm/atc/templates/servicemonitor.yaml` — Optional ServiceMonitor (`monitoring.coreos.com/v1`), gated on `metrics.enabled && metrics.serviceMonitor.enabled`; includes label selector for Prometheus discovery
- `deploy/helm/atc/templates/tests/test-connection.yaml` — Helm test hook Pod with restricted Pod Security Standards; validates Service connectivity; excluded from charts via `helm.sh/hook: test` annotation
- `deploy/helm/atc/tests/values-*.yaml` — CI values matrix for Phase 5 (persistence, gateway, metrics, ingress, and combinations); excluded from chart tarball by `.helmignore /tests/` anchor
