# Changelog

## [0.2.0](https://github.com/bojanrajkovic/atc/compare/v0.1.0...v0.2.0) (2026-06-04)

First feature release of ATC — a real-time GitHub Actions dashboard with a Rust/Axum backend and a Svelte 5 frontend.

### Features

#### Ingestion & server

- GitHub webhook ingestion with HMAC-SHA256 verification, WebSocket event streaming, and a REST state snapshot ([#20](https://github.com/bojanrajkovic/atc/issues/20), [#21](https://github.com/bojanrajkovic/atc/issues/21))
- Core domain model and state machine for workflow runs, jobs, and steps ([#18](https://github.com/bojanrajkovic/atc/issues/18))
- Attempt-aware run and job state for GitHub re-runs ([#303](https://github.com/bojanrajkovic/atc/issues/303))
- Protocol version handshake and `GoingAway` close envelope ([#227](https://github.com/bojanrajkovic/atc/issues/227))
- Cooperative graceful shutdown with background-task supervision ([#81](https://github.com/bojanrajkovic/atc/issues/81))

#### Persistence (PostgreSQL)

- Persist workflow run and job state to PostgreSQL — connection pool, schema migrations, and a `readyz` probe ([#48](https://github.com/bojanrajkovic/atc/issues/48), [#49](https://github.com/bojanrajkovic/atc/issues/49))
- Transactional outbox with LISTEN/NOTIFY drain, cutting `/v1/state` and `/v1/ws` over to the PG-backed read path ([#51](https://github.com/bojanrajkovic/atc/issues/51), [#52](https://github.com/bojanrajkovic/atc/issues/52), [#54](https://github.com/bojanrajkovic/atc/issues/54))
- Outbox retention sweep with heartbeat and metrics ([#67](https://github.com/bojanrajkovic/atc/issues/67), [#192](https://github.com/bojanrajkovic/atc/issues/192))
- Split persistence into focused crates — `atc-wire`, `atc-persist`, `atc-store-pg`, `atc-store-mem` ([#198](https://github.com/bojanrajkovic/atc/issues/198), [#199](https://github.com/bojanrajkovic/atc/issues/199), [#200](https://github.com/bojanrajkovic/atc/issues/200))

#### Runner pools & configuration

- Configurable runner-pool capacity per label set, including unbounded (`capacity: null`) ([#177](https://github.com/bojanrajkovic/atc/issues/177), [#206](https://github.com/bojanrajkovic/atc/issues/206))
- Hot-reload `runner_pools` configuration without a restart ([#172](https://github.com/bojanrajkovic/atc/issues/172), [#205](https://github.com/bojanrajkovic/atc/issues/205))
- Live runner-pool stats broadcast with disambiguated runner-bar labels ([#34](https://github.com/bojanrajkovic/atc/issues/34))

#### Observability

- OpenTelemetry instrumentation — distributed tracing and metrics — plus a broader observability pass ([#91](https://github.com/bojanrajkovic/atc/issues/91), [#245](https://github.com/bojanrajkovic/atc/issues/245))

#### Frontend

- Frontend foundation: type generation, design system, stores, WebSocket client, and E2E tests ([#22](https://github.com/bojanrajkovic/atc/issues/22))
- App shell, kanban board, and settings popover ([#23](https://github.com/bojanrajkovic/atc/issues/23), [#25](https://github.com/bojanrajkovic/atc/issues/25))
- Command palette (Cmd+K), detail panel, hover-peek, and pool filter ([#41](https://github.com/bojanrajkovic/atc/issues/41))
- Roving-tabindex keyboard navigation, responsive layout, and ARIA live regions ([#43](https://github.com/bojanrajkovic/atc/issues/43), [#45](https://github.com/bojanrajkovic/atc/issues/45))
- Run cards with state-aware duration, status tokens, and density modes ([#30](https://github.com/bojanrajkovic/atc/issues/30))
- Display TTL for completed runs and jobs ([#256](https://github.com/bojanrajkovic/atc/issues/256))
- URL-based deep linking for the selected run ([#241](https://github.com/bojanrajkovic/atc/issues/241))
- Surface config-reload errors via an admin alert banner ([#230](https://github.com/bojanrajkovic/atc/issues/230))

#### Deployment (Helm)

- Helm chart with metrics and release publishing ([#14](https://github.com/bojanrajkovic/atc/issues/14))
- Multi-replica gated on PostgreSQL (sqlite/persistence removed), with pod anti-affinity, PodDisruptionBudget, HorizontalPodAutoscaler, and NetworkPolicy ([#7](https://github.com/bojanrajkovic/atc/issues/7), [#57](https://github.com/bojanrajkovic/atc/issues/57), [#86](https://github.com/bojanrajkovic/atc/issues/86), [#87](https://github.com/bojanrajkovic/atc/issues/87), [#88](https://github.com/bojanrajkovic/atc/issues/88), [#89](https://github.com/bojanrajkovic/atc/issues/89))
- Graceful-shutdown deploy surface — preStop hook and readiness-probe coordination ([#85](https://github.com/bojanrajkovic/atc/issues/85))
- Bundled Grafana dashboard via sidecar with operator discovery ([#224](https://github.com/bojanrajkovic/atc/issues/224))
- Restructure `existingSecret` into per-credential blocks ([#242](https://github.com/bojanrajkovic/atc/issues/242))
- Publish chart via chart-releaser to GitHub Pages ([#92](https://github.com/bojanrajkovic/atc/issues/92))

### Bug Fixes

- **state-machine:** allow Queued → Completed job transition for the GitHub cancellation path ([#144](https://github.com/bojanrajkovic/atc/issues/144))
- **runner-pools:** exclude orphaned Queued jobs from completed runs in pool stats ([cb2e962](https://github.com/bojanrajkovic/atc/commit/cb2e962c794ee4a1e81702faae6fd15583df0bf0))
- **server:** support HTTPS OTLP endpoints and honor spec path semantics ([#93](https://github.com/bojanrajkovic/atc/issues/93))
- **metrics:** source `atc_build_info` version from `VERGEN_GIT_DESCRIBE` ([96abae4](https://github.com/bojanrajkovic/atc/commit/96abae495b90cfa35494428034f2d49b37fa135c))
- **frontend:** swap Toggle for Switch in the settings popover ([#72](https://github.com/bojanrajkovic/atc/issues/72))

### Performance Improvements

- **frontend:** migrate run-store Maps to `SvelteMap` for per-key reactivity ([#26](https://github.com/bojanrajkovic/atc/issues/26), [#33](https://github.com/bojanrajkovic/atc/issues/33))
- **frontend:** pace WebSocket batch injection across animation-frame boundaries ([#46](https://github.com/bojanrajkovic/atc/issues/46), [#204](https://github.com/bojanrajkovic/atc/issues/204))
- **atc-server:** consolidate integration tests into a single binary ([#83](https://github.com/bojanrajkovic/atc/issues/83))

_Routine dependency bumps (Renovate) are omitted from this curated changelog; see the git history for the full list._
