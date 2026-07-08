# Changelog

## [0.4.1](https://github.com/bojanrajkovic/atc/compare/v0.4.0...v0.4.1) (2026-07-08)


### Bug Fixes

* **server:** match lightweight git tags in VERGEN_GIT_DESCRIBE ([c4bb40b](https://github.com/bojanrajkovic/atc/commit/c4bb40b858e27bf9c3cbfab768eaae4732362394))

## [0.4.0](https://github.com/bojanrajkovic/atc/compare/v0.3.0...v0.4.0) (2026-07-07)


### Features

* **ci:** add per-crate Codecov components for backend coverage display ([78d98bd](https://github.com/bojanrajkovic/atc/commit/78d98bd14ee2357ac0ef3d83b7411c54c6fcabb5))
* **server:** trace HTTP requests, in both directions, through the OTel pipeline ([#521](https://github.com/bojanrajkovic/atc/issues/521)) ([df80636](https://github.com/bojanrajkovic/atc/commit/df80636a62c3e03790624d6c99a441ac5163bfd5))


### Bug Fixes

* **backend:** isolate the DB-outage test and cap pool acquire_timeout at 5s ([#517](https://github.com/bojanrajkovic/atc/issues/517)) ([f9fce38](https://github.com/bojanrajkovic/atc/commit/f9fce3805840a5170f02def17f2cd20e60dff4ba))
* **server:** stop pulling a full state snapshot to list known repo IDs ([#515](https://github.com/bojanrajkovic/atc/issues/515)) ([757f1ef](https://github.com/bojanrajkovic/atc/commit/757f1ef50749d549e58df82b790dc230b2146b07))
* **test:** reap stale per-test databases in reused PG containers ([#514](https://github.com/bojanrajkovic/atc/issues/514)) ([79a4a11](https://github.com/bojanrajkovic/atc/commit/79a4a11c789e0385fd717a71bab371789de19444))
* **tests:** drop over-asserting dedup-counter check from row-lock test ([#520](https://github.com/bojanrajkovic/atc/issues/520)) ([1b1613c](https://github.com/bojanrajkovic/atc/commit/1b1613cf4803969c3cfa81d6ecbef4ca4db2c491)), closes [#519](https://github.com/bojanrajkovic/atc/issues/519)


### Performance Improvements

* **release:** reuse binary-matrix artifact instead of recompiling container ([#512](https://github.com/bojanrajkovic/atc/issues/512)) ([0db9a59](https://github.com/bojanrajkovic/atc/commit/0db9a5962a14467540d440caa01ee482ad3519c3))

## [0.3.0](https://github.com/bojanrajkovic/atc/compare/v0.2.0...v0.3.0) (2026-07-06)


### Features

* **core,wire:** carry repo_id on WorkflowRun incl. placeholder runs ([#477](https://github.com/bojanrajkovic/atc/issues/477)) ([8cb5976](https://github.com/bojanrajkovic/atc/commit/8cb59760eeacb8b48e0a7b2f4736fc9f529f8f22)), closes [#450](https://github.com/bojanrajkovic/atc/issues/450)
* **frontend:** 401-aware connection states ([#494](https://github.com/bojanrajkovic/atc/issues/494)) ([706fd78](https://github.com/bojanrajkovic/atc/commit/706fd78706ebf61a111e5511afe2e3bacc3cae93)), closes [#462](https://github.com/bojanrajkovic/atc/issues/462)
* **frontend:** login screen, identity chrome, logout ([#495](https://github.com/bojanrajkovic/atc/issues/495)) ([ebb858d](https://github.com/bojanrajkovic/atc/commit/ebb858db6fa8f8f1d8f0d61129210dc5095ae038)), closes [#463](https://github.com/bojanrajkovic/atc/issues/463)
* **frontend:** popup-first staleness re-auth with redirect fallback ([#497](https://github.com/bojanrajkovic/atc/issues/497)) ([de5ecd7](https://github.com/bojanrajkovic/atc/commit/de5ecd7bd43b031409596ae3102c09153d8a391c)), closes [#464](https://github.com/bojanrajkovic/atc/issues/464)
* **github,core:** parse repository.id and carry repo_id on event envelopes ([#474](https://github.com/bojanrajkovic/atc/issues/474)) ([f870c9b](https://github.com/bojanrajkovic/atc/commit/f870c9b50abb8f6de1d00b5e63e79aa1f4e5398c)), closes [#449](https://github.com/bojanrajkovic/atc/issues/449)
* **helm:** auth.github values, secret ref, and template guards ([#484](https://github.com/bojanrajkovic/atc/issues/484)) ([87e6737](https://github.com/bojanrajkovic/atc/commit/87e6737f0e0859ae4ce0eda2062a1ed91d79158c)), closes [#466](https://github.com/bojanrajkovic/atc/issues/466)
* log ping / parse-failure / state-transition webhook events ([#372](https://github.com/bojanrajkovic/atc/issues/372)) ([f0d485d](https://github.com/bojanrajkovic/atc/commit/f0d485d82d8efdcb83413185c85719831b37723b))
* **observability:** auth flow spans and metrics ([#506](https://github.com/bojanrajkovic/atc/issues/506)) ([bd171d1](https://github.com/bojanrajkovic/atc/commit/bd171d16a8441fa76f63dbefef6748f03aab8ae5)), closes [#469](https://github.com/bojanrajkovic/atc/issues/469)
* **server:** add AuthContext extractor and 401 reason contract ([#485](https://github.com/bojanrajkovic/atc/issues/485)) ([41c6def](https://github.com/bojanrajkovic/atc/commit/41c6def2a4a840fb6f029e9a82ea4cc01c8f08a2)), closes [#458](https://github.com/bojanrajkovic/atc/issues/458)
* **server:** add GitHub OAuth login and callback endpoints ([#480](https://github.com/bojanrajkovic/atc/issues/480)) ([2a387f3](https://github.com/bojanrajkovic/atc/commit/2a387f3fb66c8c788c2e7387782e8dfc18c46ba9)), closes [#455](https://github.com/bojanrajkovic/atc/issues/455)
* **server:** add logout and whoami endpoints ([#481](https://github.com/bojanrajkovic/atc/issues/481)) ([327af7d](https://github.com/bojanrajkovic/atc/commit/327af7d6c39c0e9d70e3191db190290f80fe2dfe)), closes [#456](https://github.com/bojanrajkovic/atc/issues/456)
* **server:** add staleness sweep for stuck non-terminal runs/jobs ([#445](https://github.com/bojanrajkovic/atc/issues/445)) ([66f724c](https://github.com/bojanrajkovic/atc/commit/66f724cb33c0cbe1207aae9cc4537c4a6bb96b74))
* **server:** auth config section + boot validation ([#476](https://github.com/bojanrajkovic/atc/issues/476)) ([5c1349a](https://github.com/bojanrajkovic/atc/commit/5c1349a72f6f47326da66583e938bb66dc0be697)), closes [#453](https://github.com/bojanrajkovic/atc/issues/453)
* **server:** filter /v1/state by session repo set ([#490](https://github.com/bojanrajkovic/atc/issues/490)) ([20169f8](https://github.com/bojanrajkovic/atc/commit/20169f811e8dc195c978d53755e853f669ea4bc9)), closes [#459](https://github.com/bojanrajkovic/atc/issues/459)
* **server:** widen ADR-0014 repo visibility to public GitHub repos ([#504](https://github.com/bojanrajkovic/atc/issues/504)) ([1c2936c](https://github.com/bojanrajkovic/atc/commit/1c2936c290b7b00ff648e0bcbc8715f2fa6eab76))
* **store-pg:** persist repo_id on runs ([#479](https://github.com/bojanrajkovic/atc/issues/479)) ([806eb0d](https://github.com/bojanrajkovic/atc/commit/806eb0d7df075a5d889e1515ebf9229511b913fb)), closes [#451](https://github.com/bojanrajkovic/atc/issues/451)
* **store-pg:** session and auth-flow storage ([#478](https://github.com/bojanrajkovic/atc/issues/478)) ([90b1783](https://github.com/bojanrajkovic/atc/commit/90b1783942359baed354670fcb773aedb3e4d93f)), closes [#454](https://github.com/bojanrajkovic/atc/issues/454)


### Bug Fixes

* **auth:** drop broken Basic-auth from public-repo visibility check ([#509](https://github.com/bojanrajkovic/atc/issues/509)) ([32bd29b](https://github.com/bojanrajkovic/atc/commit/32bd29bc35424c9890dec8a820b9506b3686f693))
* **deps:** update rust crate chrono to v0.4.45 ([#348](https://github.com/bojanrajkovic/atc/issues/348)) ([3b197b0](https://github.com/bojanrajkovic/atc/commit/3b197b021cfdfffca518015447251d5ec350ceca))
* **deps:** update rust crate http to v1.4.2 ([#361](https://github.com/bojanrajkovic/atc/issues/361)) ([af24385](https://github.com/bojanrajkovic/atc/commit/af243856073f2457e33ca2bcaac1b3a893411488))
* **deps:** update rust crate uuid to v1.23.3 ([#376](https://github.com/bojanrajkovic/atc/issues/376)) ([26ca908](https://github.com/bojanrajkovic/atc/commit/26ca908292f1138bba71333c65ce1a6f5b9028b9))
* **deps:** update rust crate uuid to v1.23.4 ([#420](https://github.com/bojanrajkovic/atc/issues/420)) ([3abccf0](https://github.com/bojanrajkovic/atc/commit/3abccf09b492c1d1b4939424e720f3ecce4d8ab0))
* **deps:** update tower ecosystem to v0.7.0 ([#389](https://github.com/bojanrajkovic/atc/issues/389)) ([59f38d4](https://github.com/bojanrajkovic/atc/commit/59f38d42391fbb1725928b6f190017399f39deec))
* **docs:** correct coverage rule to include public-repo visibility widening ([#508](https://github.com/bojanrajkovic/atc/issues/508)) ([917374d](https://github.com/bojanrajkovic/atc/commit/917374d94674221e51241bf6ec8d39ca7c1aa0be))
* **frontend:** auto-attempt popup re-auth for auth_required, never disable the login link ([#499](https://github.com/bojanrajkovic/atc/issues/499)) ([891b401](https://github.com/bojanrajkovic/atc/commit/891b401e5d65ee7cf5cece65b5d1aaed68358aa2)), closes [#498](https://github.com/bojanrajkovic/atc/issues/498)
* **helm:** permit Helm-injected global values in root schema ([#511](https://github.com/bojanrajkovic/atc/issues/511)) ([8876267](https://github.com/bojanrajkovic/atc/commit/887626759e73b829d287a00e243077a75d1b99ec)), closes [#342](https://github.com/bojanrajkovic/atc/issues/342)
* **observability:** rename error tracing field to error.message ([#442](https://github.com/bojanrajkovic/atc/issues/442)) ([9b1cdb0](https://github.com/bojanrajkovic/atc/commit/9b1cdb051b4d45226ec687a30e463416a8a47765))
* **server:** send a User-Agent header on GitHub API requests ([#483](https://github.com/bojanrajkovic/atc/issues/483)) ([add81d9](https://github.com/bojanrajkovic/atc/commit/add81d9cf8f6fc078085fe1b6c7d74c98a14035d))
* **store:** thread repo_id through staleness-sweep completion envelopes ([#493](https://github.com/bojanrajkovic/atc/issues/493)) ([b84628d](https://github.com/bojanrajkovic/atc/commit/b84628d865882836b3b69de528040d4aeda262dc))

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
