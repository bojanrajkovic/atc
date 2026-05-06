# ATC Helm Chart

> **Note:** Values reference section is maintained manually; see `values.yaml` for authoritative field documentation.

## Overview

ATC (Actions Traffic Control) is a real-time GitHub Actions dashboard. This Helm chart packages the ATC server for deployment to any conformant Kubernetes cluster (≥1.29). It creates a single Deployment with a ClusterIP Service and ServiceAccount, with optional routing resources (Ingress, Gateway API HTTPRoute) and observability resources (ServiceMonitor) that can be toggled independently. The chart is shipped with secure-by-default Pod Security Standards (restricted) enforcement baked in.

For architecture details and design decisions, see [`docs/architecture/deployment.md`](../../../docs/architecture/deployment.md).

## Install from OCI Registry

The chart is published as an OCI artifact to `ghcr.io/bojanrajkovic/charts/atc`:

```bash
helm install atc oci://ghcr.io/bojanrajkovic/charts/atc --version <version>
```

## Install from Local Source

For development:

```bash
helm install atc ./deploy/helm/atc
```

## Two Supported Storage Modes

### Ephemeral / Demo (Default)

Stateless mode with no external database. Best for first-touch, homelab exploration, and testing. Single-replica only.

```bash
helm install atc oci://ghcr.io/bojanrajkovic/charts/atc
```

**Update strategy:** RollingUpdate (zero downtime; state lost on pod restart)

### External Postgres

ATC connects to an operator-managed Postgres instance. Required for any `replicaCount > 1` configuration. Suitable for production, multi-replica homelab, or work clusters with an operated Postgres service.

Store the connection string in a Kubernetes Secret:

```bash
kubectl create secret generic atc-db --from-literal=database_url=postgres://user:pass@host:5432/atc
```

Then install the chart:

```bash
helm install atc oci://ghcr.io/bojanrajkovic/charts/atc \
  --set existingSecret.name=atc-db
```

Alternatively, pass the URL directly (not recommended for production):

```bash
helm install atc oci://ghcr.io/bojanrajkovic/charts/atc \
  --set config.databaseUrl=postgres://user:pass@host:5432/atc
```

**Update strategy:** RollingUpdate (zero downtime)

## Multi-replica with External Postgres

`replicaCount > 1` requires a PostgreSQL connection string via either `config.databaseUrl` or `existingSecret.name`+`existingSecret.databaseUrlKey`. The chart enforces this at template-render time with a `{{ fail }}` guard. Per ADR 0003 D3: ephemeral in-memory mode is single-replica only.

```bash
helm install atc oci://ghcr.io/bojanrajkovic/charts/atc \
  --set replicaCount=2 \
  --set existingSecret.name=atc-db
```

**Sticky sessions are NOT required** and are discouraged outside specific cost-tuning scenarios. Each replica serves `/v1/state` and `/v1/ws` independently; clients reconnect-then-snapshot via `/v1/state`+`lastSeq` and resume from any replica.

For the full multi-replica smoke-test runbook (`kubectl`/`curl` commands, a Node WebSocket tap at `scripts/ws-tap.js`, single-delivery and snapshot-convergence assertions, and the re-run reset procedure), see the [Multi-replica smoke test](../../../docs/architecture/deployment.md#multi-replica-smoke-test) section in `deployment.md`.

## Values Reference

See `values.yaml` for the complete list of configurable parameters. Every field is documented inline with comments explaining the purpose and valid values.

Key sections:
- `replicaCount` — Number of Pod replicas. Values > 1 require an external Postgres connection string (chart-render-time guard).
- `image` — Container image repository, tag, and pull policy
- `config` — ATC application configuration (HTTP addr, metrics addr, database URL, listener URL, logging)
- `existingSecret` — Reference an existing Secret for the database URL and listener URL
- `metrics` — Whether to expose the metrics port (9090) on the Service; optional ServiceMonitor
- `serviceAccount` — ServiceAccount creation and annotations
- `service` — Service type and port configuration
- `ingress` / `gateway` — Optional routing configuration

## Restricted Pod Security Standards

The chart enforces Kubernetes [Restricted Pod Security Standards](https://kubernetes.io/docs/concepts/security/pod-security-standards/#restricted) by default. This means:

- All containers run as a non-root user (UID 65532) with a read-only root filesystem
- Privilege escalation is blocked
- All Linux capabilities are dropped
- seccomp profile is set to RuntimeDefault
- An emptyDir volume is mounted at `/tmp` (required for read-only root)

The chart works out of the box in namespaces with `pod-security.kubernetes.io/enforce: restricted` label set.

## Upgrading

To upgrade the chart to a new version:

```bash
helm upgrade atc oci://ghcr.io/bojanrajkovic/charts/atc --version <new-version>
```

If you're upgrading from local source during development:

```bash
helm upgrade atc ./deploy/helm/atc
```
