# ATC Helm Chart

> **Note:** Values reference section is maintained manually; see `values.yaml` for authoritative field documentation.

## Overview

ATC (Actions Traffic Control) is a real-time GitHub Actions dashboard. This Helm chart packages the ATC server for deployment to any conformant Kubernetes cluster (≥1.29). It creates a single Deployment with a ClusterIP Service and ServiceAccount, with optional routing resources (Ingress, Gateway API HTTPRoute) that can be toggled independently. Observability is exported via OpenTelemetry (OTLP/HTTP) when enabled — operators run a collector that exposes whatever scrape format their backend prefers. The chart is shipped with secure-by-default Pod Security Standards (restricted) enforcement baked in.

For architecture details and design decisions, see [`docs/architecture/deployment.md`](../../../docs/architecture/deployment.md).

> **No built-in authentication.** ATC does not gate the SPA, `/v1/state`, or `/v1/ws`. Deploy the chart behind a trusted network surface (private VPC, VPN, Tailscale) or an authenticating reverse proxy. See [`docs/operator/authentication.md`](../../../docs/operator/authentication.md) for per-proxy recipes (Pomerium, oauth2-proxy, Authelia, Cloudflare Access) and the webhook-endpoint split.

> **Pre-1.0 stability.** The chart is still on the `0.x` line; minor version bumps may carry breaking changes to values keys. Check `CHANGELOG.md` before upgrading.

## Install from GitHub Pages (Recommended)

The chart is published as a classic HTTP Helm repo on GitHub Pages. This is the recommended path for consumers without GHCR authentication:

```bash
helm repo add atc https://bojanrajkovic.github.io/atc/charts
helm repo update
helm install atc atc/atc --version <version>
```

## Install from OCI Registry

The same chart is also published as an OCI artifact to `ghcr.io/bojanrajkovic/charts/atc` for OCI-native workflows:

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
- `config` — ATC application configuration (HTTP addr, database URL, listener URL, logging)
- `existingSecret` — Reference an existing Secret for the database URL and listener URL
- `otel` — OpenTelemetry export. Set `enabled: true` and point `endpoint` at an OTLP/HTTP collector to inject the spec-standard `OTEL_*` env vars; defaults to disabled
- `serviceAccount` — ServiceAccount creation and annotations
- `service` — Service type and port configuration
- `ingress` / `gateway` — Optional routing configuration
- `runnerPools` — Operator-declared runner-pool capacities. When non-empty, the chart renders a `ConfigMap` mounted read-only at `/etc/atc/config.yaml`; `atc-server` reads the file at startup and surfaces the declared capacities on `/v1/state` so the frontend can render saturation bars for bounded pools and a distinct affordance for pools declared unbounded. Default `[]` keeps in-memory dev mode and existing deployments byte-identical.

### Runner-pool capacities

Declare each pool's `capacity` per label set. Use an integer for bounded pools; use `null` for pools without a renderable ceiling (e.g. ARC `AutoscalingRunnerSet` without `maxRunners`, or GitHub-hosted runners):

```yaml
runnerPools:
  - labels: [self-hosted, linux, x64]
    capacity: 10            # bounded — renders running/10 with a saturation bar
  - labels: [ubuntu-latest]
    capacity: null          # unbounded — renders running with an ∞ affordance
```

`labels` is a non-empty array of unique strings (server-side canonicalized to sorted + deduplicated form). `capacity` is required on every entry: an integer ≥ 1 declares a bounded pool, `null` declares the pool unbounded. Omitting the `capacity` key is rejected at server startup — `null` is the canonical way to declare unboundedness.

`values.schema.json` rejects malformed entries (empty labels, `capacity: 0`, unknown sibling keys) at `helm install` / `helm upgrade` time; the server additionally rejects duplicate canonicalized label sets across the list at startup and rejects the missing-`capacity`-key case via its custom `Deserialize` impl. Empty list (the default) renders no `ConfigMap` and no volume mount. Hot-reload is not supported — pool changes require a Pod restart in this chart version.

### OpenTelemetry export

When `otel.enabled: true`, the chart injects the spec-standard `OTEL_*` env vars into the container:

| Values key | Env var | Purpose |
|------------|---------|---------|
| `otel.endpoint` | `OTEL_EXPORTER_OTLP_ENDPOINT` | OTLP/HTTP collector URL (e.g. `http://otel-collector.observability:4318`). Required when enabled. |
| `otel.serviceName` | `OTEL_SERVICE_NAME` | Resource attribute identifying the service. Defaults to `"atc"`. |
| `otel.resourceAttributes` | `OTEL_RESOURCE_ATTRIBUTES` | Comma-separated `key=value` pairs appended after the auto-injected k8s identifiers. E.g. `deployment.environment=production,service.namespace=ingest`. |
| `otel.sampler` | `OTEL_TRACES_SAMPLER` | Trace sampler. Defaults to `parentbased_always_on`. |
| `otel.samplerArg` | `OTEL_TRACES_SAMPLER_ARG` | Sampler argument (decimal in `[0, 1]`, e.g. `"0.1"` for 10% root sampling with `parentbased_traceidratio`). REQUIRED non-empty when `otel.sampler` is `traceidratio` or `parentbased_traceidratio` — render-time guard fails otherwise. |

When `otel.enabled: true`, the chart also wires four downward-API env vars (`OTEL_K8S_POD_NAME`, `OTEL_K8S_POD_NAMESPACE`, `OTEL_K8S_POD_UID`, `OTEL_K8S_NODE_NAME`) and prepends `k8s.pod.name`, `k8s.namespace.name`, `k8s.pod.uid`, `k8s.node.name`, and `k8s.deployment.name` to `OTEL_RESOURCE_ATTRIBUTES` so per-pod identity surfaces in Tempo and Mimir without requiring per-environment values overrides. The operator-supplied `otel.resourceAttributes` value is appended after this prefix; an explicit `k8s.*` override wins because the OTel SDK takes the last value for duplicate keys.

OTLP transport is HTTP/protobuf only. There is no `protocol` key — gRPC is out of scope and would require an opt-in build of `atc-server`.

Setting `otel.enabled: true` with an empty `otel.endpoint` is rejected at template render time — `atc-server` treats an empty endpoint as disabled, so a blank value would silently turn telemetry off.

When `otel.enabled: false` (the default) no `OTEL_*` env vars are injected and the OTel SDK is never initialized in the container.

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
