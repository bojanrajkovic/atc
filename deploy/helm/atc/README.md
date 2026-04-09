# ATC Helm Chart

> **Note:** Values reference section is maintained manually; see `values.yaml` for authoritative field documentation.

## Overview

ATC (Actions Traffic Control) is a real-time GitHub Actions dashboard. This Helm chart packages the ATC server for deployment to any conformant Kubernetes cluster (≥1.29). It creates a single Deployment with a ClusterIP Service and ServiceAccount, with optional resources (Ingress, Gateway API HTTPRoute, PersistentVolumeClaim) that can be toggled independently. The chart is shipped with secure-by-default Pod Security Standards (restricted) enforcement baked in.

For architecture details and design decisions, see [`docs/architecture/deployment.md`](../../docs/architecture/deployment.md).

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

## Three Supported Storage Modes

### Ephemeral / Demo (Default)

Stateless mode with no external database. Best for first-touch, homelab exploration, and testing.

```bash
helm install atc oci://ghcr.io/bojanrajkovic/charts/atc
```

**Update strategy:** RollingUpdate (zero downtime)

### Local SQLite with Persistence

ATC stores state in a SQLite file on a PersistentVolume. Suitable for single-instance homelab or work cluster without an external database.

```bash
helm install atc oci://ghcr.io/bojanrajkovic/charts/atc \
  --set persistence.enabled=true \
  --set config.databaseUrl=sqlite:///var/lib/atc/atc.db
```

**Update strategy:** Recreate (brief downtime; ReadWriteOnce volume constraint)

**Note:** Setting `persistence.enabled=false` with a `sqlite://` database URL outside `/tmp` will fail at render time with an explicit error message.

### External Postgres

ATC connects to an operator-managed Postgres instance. Ideal for work cluster with an operated Postgres service.

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

**Update strategy:** RollingUpdate (zero downtime; no local volume)

## Values Reference

See `values.yaml` for the complete list of configurable parameters. Every field is documented inline with comments explaining the purpose and valid values.

Key sections:
- `replicaCount` — Number of Pod replicas (incompatible with persistence when > 1)
- `image` — Container image repository, tag, and pull policy
- `config` — ATC application configuration (HTTP addr, metrics addr, database URL, logging)
- `persistence` — PersistentVolumeClaim settings (enabled, storage class, size, mount path)
- `metrics` — Whether to expose the metrics port (9090) on the Service
- `serviceAccount` — ServiceAccount creation and annotations
- `podSecurityContext` / `securityContext` — Pod and container security settings
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
