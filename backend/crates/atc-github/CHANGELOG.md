# Changelog

## [0.2.0](https://github.com/bojanrajkovic/atc/compare/v0.1.0...v0.2.0) (2026-06-04)


### Features

* add repo skeleton with Rust backend and Svelte frontend ([19dc156](https://github.com/bojanrajkovic/atc/commit/19dc156d4ad3959bb232ac245430ab6eb7664291))
* **atc-github:** implement GitHub webhook parsing and HMAC verification ([#20](https://github.com/bojanrajkovic/atc/issues/20)) ([c24dc4e](https://github.com/bojanrajkovic/atc/commit/c24dc4e0861d8be97ee2d7ab023161af5f3cbf11))
* broadcast live runner pool stats and disambiguate runner bar labels ([#34](https://github.com/bojanrajkovic/atc/issues/34)) ([8e0f2af](https://github.com/bojanrajkovic/atc/commit/8e0f2afa9ece4e1c02fb6074f53c94e99e9fcd55))
* handle GitHub re-runs with attempt-aware run and job state ([#303](https://github.com/bojanrajkovic/atc/issues/303)) ([c7eb703](https://github.com/bojanrajkovic/atc/commit/c7eb703e8a856d107d9584fb85dbc31db46c1e0e))
* implement frontend foundation — type generation, design system, stores, WebSocket client, E2E tests ([2a7e416](https://github.com/bojanrajkovic/atc/commit/2a7e41675883dedeeac5e2016d7e4bb89a62f50d))
* implement frontend foundation — type generation, design system, stores, WebSocket client, E2E tests ([#22](https://github.com/bojanrajkovic/atc/issues/22)) ([2a7e416](https://github.com/bojanrajkovic/atc/commit/2a7e41675883dedeeac5e2016d7e4bb89a62f50d))
* observability pass ([#245](https://github.com/bojanrajkovic/atc/issues/245)) ([ebf6118](https://github.com/bojanrajkovic/atc/commit/ebf611851e76f73768a3fea04d5f14854751854c))
* **server:** introduce OpenTelemetry instrumentation (tracing and metrics) ([#91](https://github.com/bojanrajkovic/atc/issues/91)) ([feb2858](https://github.com/bojanrajkovic/atc/commit/feb285880c8a253cf17946484e92b4421fe16439))
* wire server endpoints — webhook ingestion, WebSocket streaming, REST state snapshot ([#21](https://github.com/bojanrajkovic/atc/issues/21)) ([4cb6e55](https://github.com/bojanrajkovic/atc/commit/4cb6e558ee91d1881bf719c19477fc12d45c17bf))


### Bug Fixes

* **ci:** drop stale version pin on atc-core dep in atc-github ([#27](https://github.com/bojanrajkovic/atc/issues/27)) ([321ddf7](https://github.com/bojanrajkovic/atc/commit/321ddf7fc5394b79e842283c61f3f87459b09237))
* **deps:** pin dependencies ([#97](https://github.com/bojanrajkovic/atc/issues/97)) ([4e8ff1c](https://github.com/bojanrajkovic/atc/commit/4e8ff1cb982864af989097dad28aed9358af2e86))
* **deps:** update rust crate const-hex to v1.19.0 ([#142](https://github.com/bojanrajkovic/atc/issues/142)) ([ba0c3c9](https://github.com/bojanrajkovic/atc/commit/ba0c3c9dfcb79f5bbccdd11dde9b1dbeea3cd204))
* **deps:** update rust crate const-hex to v1.19.1 ([#285](https://github.com/bojanrajkovic/atc/issues/285)) ([e7dd727](https://github.com/bojanrajkovic/atc/commit/e7dd727e8815b66e15835bf9b51eb9b6205045ee))
* **deps:** update rust crate serde_json to v1.0.150 ([#276](https://github.com/bojanrajkovic/atc/issues/276)) ([6702bd8](https://github.com/bojanrajkovic/atc/commit/6702bd8b6cbd873086a23899bbd647a68b750d1c))
