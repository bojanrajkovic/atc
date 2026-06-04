# Changelog

## [0.2.0](https://github.com/bojanrajkovic/atc/compare/v0.1.0...v0.2.0) (2026-06-04)


### Features

* add repo skeleton with Rust backend and Svelte frontend ([19dc156](https://github.com/bojanrajkovic/atc/commit/19dc156d4ad3959bb232ac245430ab6eb7664291))
* align the state cursor contract and derive runner pools in the frontend ([4d01b1b](https://github.com/bojanrajkovic/atc/commit/4d01b1bc7928a2eebd8295ad14e58485a471d9fa))
* **atc-core:** implement core domain model ([#18](https://github.com/bojanrajkovic/atc/issues/18)) ([7e36e92](https://github.com/bojanrajkovic/atc/commit/7e36e9291fdb957052f65bfacdd97658f4f1dc2a))
* **atc-github:** implement GitHub webhook parsing and HMAC verification ([#20](https://github.com/bojanrajkovic/atc/issues/20)) ([c24dc4e](https://github.com/bojanrajkovic/atc/commit/c24dc4e0861d8be97ee2d7ab023161af5f3cbf11))
* broadcast live runner pool stats and disambiguate runner bar labels ([#34](https://github.com/bojanrajkovic/atc/issues/34)) ([8e0f2af](https://github.com/bojanrajkovic/atc/commit/8e0f2afa9ece4e1c02fb6074f53c94e99e9fcd55))
* configurable runner pool capacity per label set ([#177](https://github.com/bojanrajkovic/atc/issues/177)) ([5cba11e](https://github.com/bojanrajkovic/atc/commit/5cba11efaa298b2aa5e04452774a2539ceb5a36e))
* handle GitHub re-runs with attempt-aware run and job state ([#303](https://github.com/bojanrajkovic/atc/issues/303)) ([c7eb703](https://github.com/bojanrajkovic/atc/commit/c7eb703e8a856d107d9584fb85dbc31db46c1e0e))
* implement app shell with top bar, runner indicators, and settings popover ([#23](https://github.com/bojanrajkovic/atc/issues/23)) ([d75fed9](https://github.com/bojanrajkovic/atc/commit/d75fed9bd0ab5b4d7ea8a9fe901c579d4b79c59e))
* implement frontend foundation — type generation, design system, stores, WebSocket client, E2E tests ([2a7e416](https://github.com/bojanrajkovic/atc/commit/2a7e41675883dedeeac5e2016d7e4bb89a62f50d))
* implement frontend foundation — type generation, design system, stores, WebSocket client, E2E tests ([#22](https://github.com/bojanrajkovic/atc/issues/22)) ([2a7e416](https://github.com/bojanrajkovic/atc/commit/2a7e41675883dedeeac5e2016d7e4bb89a62f50d))
* observability pass ([#245](https://github.com/bojanrajkovic/atc/issues/245)) ([ebf6118](https://github.com/bojanrajkovic/atc/commit/ebf611851e76f73768a3fea04d5f14854751854c))
* **persist:** extract atc-store-mem crate ([#169](https://github.com/bojanrajkovic/atc/issues/169)) ([#199](https://github.com/bojanrajkovic/atc/issues/199)) ([260bc5f](https://github.com/bojanrajkovic/atc/commit/260bc5f8dfc249614ee8a92a87943ffdf86af702))
* **runner-pools:** allow unbounded capacity via `capacity: null` ([#206](https://github.com/bojanrajkovic/atc/issues/206)) ([7d45fe0](https://github.com/bojanrajkovic/atc/commit/7d45fe0cc5df4a63d17e077aae03efe91daf8e61))
* **server:** persist workflow run and job state to PostgreSQL ([#49](https://github.com/bojanrajkovic/atc/issues/49)) ([b7f8614](https://github.com/bojanrajkovic/atc/commit/b7f861458b572304335ca685f4054a1acf1cc95b))
* wire server endpoints — webhook ingestion, WebSocket streaming, REST state snapshot ([#21](https://github.com/bojanrajkovic/atc/issues/21)) ([4cb6e55](https://github.com/bojanrajkovic/atc/commit/4cb6e558ee91d1881bf719c19477fc12d45c17bf))


### Bug Fixes

* **deps:** pin dependencies ([#97](https://github.com/bojanrajkovic/atc/issues/97)) ([4e8ff1c](https://github.com/bojanrajkovic/atc/commit/4e8ff1cb982864af989097dad28aed9358af2e86))
* **deps:** update rust crate serde_json to v1.0.150 ([#276](https://github.com/bojanrajkovic/atc/issues/276)) ([6702bd8](https://github.com/bojanrajkovic/atc/commit/6702bd8b6cbd873086a23899bbd647a68b750d1c))
* **state-machine:** allow Queued → Completed job transition for GitHub cancellation path ([#144](https://github.com/bojanrajkovic/atc/issues/144)) ([fa78e48](https://github.com/bojanrajkovic/atc/commit/fa78e489206816a6ddda6a21ed93ac86a82f8dea))
