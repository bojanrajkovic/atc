# ATC — Actions Traffic Control

Real-time GitHub Actions dashboard. One pane of glass across every org you control, driven by webhooks (no polling, no rate-limit math), with per-step progress, runner-pool saturation, and one-click jumps back to GitHub.

ATC ships as a single Rust binary that serves a Svelte 5 SPA, ingests GitHub webhooks, and pushes updates over WebSocket. State lives in either Postgres (production, multi-replica) or in-process memory (dev only).

## Operator note — authentication

**By default, ATC ships no built-in authentication.** Anyone who can reach the HTTP port can read every workflow run, job, and runner-pool view for every repository that has sent a webhook. The webhook endpoint validates HMAC-SHA256 signatures when a secret is configured; the SPA, the `GET /v1/state` snapshot, and the `GET /v1/ws` event stream do not — unless you opt into `auth.mode: github`.

Two ways to gate access: deploy ATC behind a trusted network surface or an authenticating reverse proxy (treat the dashboard as if it were the GitHub Actions tab of every repository whose webhooks land on it), **or** enable `auth.mode: github` for first-party GitHub OAuth login with per-repository authorization — users sign in with GitHub and see only the repos they can access there, no reverse proxy required.

See [`docs/operator/authentication.md`](docs/operator/authentication.md) for the full setup guide: per-proxy recipes for the no-built-in-auth path (Pomerium recommended; oauth2-proxy, Authelia + nginx/Caddy, and Cloudflare Access all work for the SPA + REST + WS), the GitHub App registration + coverage rule for `auth.mode: github`, the path-split layout that lets `/v1/webhooks/github` bypass either mode, and the cross-cutting gotchas.

## Why it exists

The hosted GitHub Actions view forces you to click into individual repos to see what's running, what's queued, and which step just failed. Third-party dashboards either poll the API (slow, rate-limited) or stopped being maintained years ago. Neither answers the question an operator actually has on a busy day: *which runner pool is saturated, and which run is currently holding it?*

ATC consumes the same webhook stream GitHub already sends to your repositories, applies the events through a pure state machine, and broadcasts the committed delta to every connected browser within a frame.

## Features

**Live kanban board** — three columns (Queued / In Progress / Completed), one card per workflow run, FLIP + crossfade animations as cards move between columns. Per-run progress bar reflects the active job's step count.

**Per-step visibility** — click a run to open the slide-over detail panel: job blocks with full step timelines, runner assignment, and duration. The panel scrolls to the selected job so deep-link semantics work across reconnects.

**Runner pool saturation** — the top bar groups jobs by runner label set and shows a capacity bar per pool. Operator declares the pool ceiling in Helm values (`runnerPools[].capacity`); bounded pools render a saturation bar (color-coded against threshold), pools declared `capacity: null` render an unbounded affordance, observed-but-undeclared pools show only a count.

**Hot-reload pool config** — edits to the `runnerPools` ConfigMap propagate live (no pod roll) within ~90 seconds end-to-end. The frontend swaps the capacity bars on the next WS frame.

**Command palette (`Cmd/Ctrl+K`)** — five-section fuzzy search across recent runs, all runs, jobs, pools, and theme commands. Activates from anywhere; stacks correctly over an open detail panel.

**Keyboard-driven** — 2D arrow / Home / End roving tabindex across the kanban grid; `Cmd/Ctrl+D` toggles dark mode; `Cmd/Ctrl+\` toggles themes. Focus stays on the same card through FLIP transitions.

**Four OKLCH themes** — warm amber, radar teal, violet, pink — each with dark and light mode. Themes are single-hue derivations against a perceptually uniform color space, so accents stay legible at every step in the lightness scale (verified against WCAG SC 1.4.3 by `frontend/src/lib/design-tokens.test.ts`).

**Single binary** — `atc-server` embeds the SPA via `rust-embed` and serves everything from one process. Distroless container image (`gcr.io/distroless/cc-debian13:nonroot`), Sigstore-attested, ~25 MB compressed.

**Multi-replica** — Postgres-backed mode uses a transactional outbox + `LISTEN/NOTIFY` drain so every replica sees every event in commit order. Replicas are symmetric: clients reconnect to any healthy peer and resume from `/v1/state` + `lastSeq`. Per-replica gap-healing ring buffer guarantees single delivery to each WebSocket.

**OpenTelemetry pipeline** — traces and metrics exported over OTLP/HTTP when `OTEL_EXPORTER_OTLP_ENDPOINT` is set; cold zero-overhead when unset. Structured logs go to stderr via `tracing-subscriber` (JSON in release builds, pretty in debug) and are not part of the OTel pipeline today — collect them through your container-log path. Includes an opt-in Grafana dashboard (`grafana_dashboard` ConfigMap or `GrafanaDashboard` CR) covering ingestion, drain pipeline, watermarks, and lifecycle.

**Graceful shutdown** — cooperative `CancellationToken` across HTTP serves, WebSocket handlers, the PG drain and listener tasks, the retention sweep, and the OTel pipeline. Aggregate budget ~13 s, sized inside `terminationGracePeriodSeconds: 30`. Pre-stop hook holds the pod through EndpointSlice / kube-proxy propagation.

## Quick start

### Try it locally

```bash
# Prerequisites: mise (https://mise.jdx.dev), Docker or OrbStack for tests.
just setup           # Install all tools, deps, and git hooks
just dev             # Start backend (cargo run) + frontend (vite) in parallel
```

The dev backend boots without Postgres in single-replica in-memory mode. Point GitHub webhooks at `http://localhost:8080/v1/webhooks/github` through smee.io (or send curl payloads from `backend/crates/atc-github/tests/fixtures/`).

### Deploy to Kubernetes

```bash
helm repo add atc https://bojanrajkovic.github.io/atc/charts

# Multi-replica needs a PostgreSQL URL. Create the secret the chart will read:
kubectl create secret generic atc-db \
  --from-literal=database_url='postgres://atc:CHANGE_ME@postgres.example/atc'

helm install atc atc/atc \
  --set existingSecret.database.name=atc-db \
  --set replicaCount=2
```

The chart is also published as an OCI artifact: `oci://ghcr.io/bojanrajkovic/charts/atc`. Both channels are tag-triggered and version-locked to the application release; only the OCI channel carries a Sigstore build-provenance attestation today (the Pages channel mirrors via `chart-releaser` without an attestation step). Pull the OCI artifact if you need to verify provenance with `gh attestation verify`. See [`deploy/helm/atc/README.md`](deploy/helm/atc/README.md) for the install paths, values reference, and [`docs/architecture/deployment.md`](docs/architecture/deployment.md) for the full operator surface (multi-replica preconditions, NetworkPolicy, HPA, PodDisruptionBudget, graceful shutdown, OTel wiring, runner-pool config, Grafana dashboard).

### Pull the container directly

```bash
docker pull ghcr.io/bojanrajkovic/atc:latest
gh attestation verify oci://ghcr.io/bojanrajkovic/atc:latest -R bojanrajkovic/atc
```

## Stack

Rust 1.94 + Axum backend, Svelte 5 + Vite + Tailwind v4 frontend, single binary, distroless container, Helm chart. Two storage modes: external Postgres (production, multi-replica) or in-process memory (dev only). See [`docs/architecture/backend-server.md`](docs/architecture/backend-server.md) for the seven-crate workspace layout, wire contracts, and shutdown model.

## Documentation

| Audience | Where |
|----------|-------|
| Operators (Helm chart, multi-replica, OTel, retention) | [`docs/architecture/deployment.md`](docs/architecture/deployment.md) |
| Helm install + values | [`deploy/helm/atc/README.md`](deploy/helm/atc/README.md) |
| Backend architecture | [`docs/architecture/backend-server.md`](docs/architecture/backend-server.md) |
| Frontend architecture | [`docs/architecture/frontend-app.md`](docs/architecture/frontend-app.md) |
| Observability (metrics + spans + OTel pipeline) | [`docs/architecture/metrics.md`](docs/architecture/metrics.md) |
| CI pipeline | [`docs/architecture/ci-pipeline.md`](docs/architecture/ci-pipeline.md) |
| Release pipeline | [`docs/architecture/release-pipeline.md`](docs/architecture/release-pipeline.md) |
| Architecture decisions (ADRs) | [`docs/architecture-decisions/`](docs/architecture-decisions/) |
| Design plans (per-feature) | [`docs/design-plans/`](docs/design-plans/) |
| Contributing, conventions, setup | [`CONTRIBUTING.md`](CONTRIBUTING.md) |

## License

[Apache-2.0](LICENSE)
