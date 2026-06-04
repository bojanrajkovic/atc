# Changelog

## [0.2.0](https://github.com/bojanrajkovic/atc/compare/v0.1.0...v0.2.0) (2026-06-04)


### Features

* add repo skeleton with Rust backend and Svelte frontend ([19dc156](https://github.com/bojanrajkovic/atc/commit/19dc156d4ad3959bb232ac245430ab6eb7664291))
* align the state cursor contract and derive runner pools in the frontend ([4d01b1b](https://github.com/bojanrajkovic/atc/commit/4d01b1bc7928a2eebd8295ad14e58485a471d9fa))
* **backend:** observe drain-shutdown outbox lag via histogram ([#84](https://github.com/bojanrajkovic/atc/issues/84)) ([a37f741](https://github.com/bojanrajkovic/atc/commit/a37f741958178c8993ea05865b932deae9a625a9))
* broadcast live runner pool stats and disambiguate runner bar labels ([#34](https://github.com/bojanrajkovic/atc/issues/34)) ([8e0f2af](https://github.com/bojanrajkovic/atc/commit/8e0f2afa9ece4e1c02fb6074f53c94e99e9fcd55))
* **config:** hot-reload runner_pools without restart ([#172](https://github.com/bojanrajkovic/atc/issues/172)) ([#205](https://github.com/bojanrajkovic/atc/issues/205)) ([3576d17](https://github.com/bojanrajkovic/atc/commit/3576d17cb389cc6e7f8073b21f2f25232453184d))
* configurable runner pool capacity per label set ([#177](https://github.com/bojanrajkovic/atc/issues/177)) ([5cba11e](https://github.com/bojanrajkovic/atc/commit/5cba11efaa298b2aa5e04452774a2539ceb5a36e))
* **frontend:** surface config-reload errors via admin alert banner ([#230](https://github.com/bojanrajkovic/atc/issues/230)) ([8767484](https://github.com/bojanrajkovic/atc/commit/87674843ba09bbb05d5c0c1a7813f26f1fad3423))
* handle GitHub re-runs with attempt-aware run and job state ([#303](https://github.com/bojanrajkovic/atc/issues/303)) ([c7eb703](https://github.com/bojanrajkovic/atc/commit/c7eb703e8a856d107d9584fb85dbc31db46c1e0e))
* helm chart, metrics, and release publishing for ATC ([#14](https://github.com/bojanrajkovic/atc/issues/14)) ([503e96f](https://github.com/bojanrajkovic/atc/commit/503e96f6b8a3f211733bcc0c78460d9f0bdf701f))
* **helm:** gate multi-replica on postgres, remove sqlite/persistence ([#7](https://github.com/bojanrajkovic/atc/issues/7)) ([#57](https://github.com/bojanrajkovic/atc/issues/57)) ([6b7d0e4](https://github.com/bojanrajkovic/atc/commit/6b7d0e4b36b8814fc2568f3ee323aa2895ce9827))
* **helm:** graceful shutdown deploy surface — preStop hook and readiness probe coordination ([#85](https://github.com/bojanrajkovic/atc/issues/85)) ([6f00d19](https://github.com/bojanrajkovic/atc/commit/6f00d19435c23b3a27134d189b89f0e6ddbcbae9))
* implement frontend foundation — type generation, design system, stores, WebSocket client, E2E tests ([2a7e416](https://github.com/bojanrajkovic/atc/commit/2a7e41675883dedeeac5e2016d7e4bb89a62f50d))
* implement frontend foundation — type generation, design system, stores, WebSocket client, E2E tests ([#22](https://github.com/bojanrajkovic/atc/issues/22)) ([2a7e416](https://github.com/bojanrajkovic/atc/commit/2a7e41675883dedeeac5e2016d7e4bb89a62f50d))
* observability pass ([#245](https://github.com/bojanrajkovic/atc/issues/245)) ([ebf6118](https://github.com/bojanrajkovic/atc/commit/ebf611851e76f73768a3fea04d5f14854751854c))
* **observability:** instrument GET /v1/state with state.snapshot and persist.read.snapshot spans ([06cc58a](https://github.com/bojanrajkovic/atc/commit/06cc58af24f8ac4265e66b86d32bcb0afb2b31ee))
* **persist:** extract atc-store-mem crate ([#169](https://github.com/bojanrajkovic/atc/issues/169)) ([#199](https://github.com/bojanrajkovic/atc/issues/199)) ([260bc5f](https://github.com/bojanrajkovic/atc/commit/260bc5f8dfc249614ee8a92a87943ffdf86af702))
* **persist:** extract atc-store-pg crate ([#169](https://github.com/bojanrajkovic/atc/issues/169)) ([#200](https://github.com/bojanrajkovic/atc/issues/200)) ([c58a55f](https://github.com/bojanrajkovic/atc/commit/c58a55f7dc67b522f5e5bb7bca13e32472a3ed5d))
* **persist:** extract atc-wire and atc-persist crates, rename SeqEvent to CommittedEvent ([#169](https://github.com/bojanrajkovic/atc/issues/169)) ([#198](https://github.com/bojanrajkovic/atc/issues/198)) ([1e0edd9](https://github.com/bojanrajkovic/atc/commit/1e0edd9a27a8962b9395b8d5776668d3e0da4342))
* protocol version handshake and GoingAway envelope ([#227](https://github.com/bojanrajkovic/atc/issues/227)) ([87c1fd6](https://github.com/bojanrajkovic/atc/commit/87c1fd6f22cd1a67ba018c3bcf0eb5571e9c925d))
* **runner-pools:** allow unbounded capacity via `capacity: null` ([#206](https://github.com/bojanrajkovic/atc/issues/206)) ([7d45fe0](https://github.com/bojanrajkovic/atc/commit/7d45fe0cc5df4a63d17e077aae03efe91daf8e61))
* **server:** add LISTEN/NOTIFY end-to-end with listener fetch-and-log stub ([#52](https://github.com/bojanrajkovic/atc/issues/52)) ([340f675](https://github.com/bojanrajkovic/atc/commit/340f6759f347e9a05dfd369ea9fc15524d3cf8a3))
* **server:** add operational metrics for the postgres drain path ([#63](https://github.com/bojanrajkovic/atc/issues/63)) ([14b6224](https://github.com/bojanrajkovic/atc/commit/14b62240cd0f286a5be25f0d082426716aebb322))
* **server:** add postgres connection pool, schema migration, and readyz probe ([#48](https://github.com/bojanrajkovic/atc/issues/48)) ([016eab8](https://github.com/bojanrajkovic/atc/commit/016eab8f75cfe6a59ca36ae7daff843a051406fa))
* **server:** add transactional outbox and reverse webhook error policy ([#51](https://github.com/bojanrajkovic/atc/issues/51)) ([2216390](https://github.com/bojanrajkovic/atc/commit/22163900f9b9b5bada58d09e52584c568dd3ee37))
* **server:** cut /v1/state and /v1/ws over to PG-backed read path ([#54](https://github.com/bojanrajkovic/atc/issues/54)) ([f367353](https://github.com/bojanrajkovic/atc/commit/f36735342ed5a5aca74bcb03368bce7c6562caaf))
* **server:** introduce OpenTelemetry instrumentation (tracing and metrics) ([#91](https://github.com/bojanrajkovic/atc/issues/91)) ([feb2858](https://github.com/bojanrajkovic/atc/commit/feb285880c8a253cf17946484e92b4421fe16439))
* **server:** outbox retention — heartbeat + sweep + metrics ([#67](https://github.com/bojanrajkovic/atc/issues/67)) ([#192](https://github.com/bojanrajkovic/atc/issues/192)) ([202b1dd](https://github.com/bojanrajkovic/atc/commit/202b1dd9b058e1f39dafdcc60dcb500d5f8e3fe6))
* **server:** persist workflow run and job state to PostgreSQL ([#49](https://github.com/bojanrajkovic/atc/issues/49)) ([b7f8614](https://github.com/bojanrajkovic/atc/commit/b7f861458b572304335ca685f4054a1acf1cc95b))
* wire server endpoints — webhook ingestion, WebSocket streaming, REST state snapshot ([#21](https://github.com/bojanrajkovic/atc/issues/21)) ([4cb6e55](https://github.com/bojanrajkovic/atc/commit/4cb6e558ee91d1881bf719c19477fc12d45c17bf))


### Bug Fixes

* **deps:** pin dependencies ([#97](https://github.com/bojanrajkovic/atc/issues/97)) ([4e8ff1c](https://github.com/bojanrajkovic/atc/commit/4e8ff1cb982864af989097dad28aed9358af2e86))
* **deps:** pin dependencies ([#98](https://github.com/bojanrajkovic/atc/issues/98)) ([8317be2](https://github.com/bojanrajkovic/atc/commit/8317be2988ec3c72849eb649c50a737d83edb78c))
* **deps:** pin dependencies ([#99](https://github.com/bojanrajkovic/atc/issues/99)) ([03d8941](https://github.com/bojanrajkovic/atc/commit/03d8941a2128e486bb829ece9b07b462af72a3a1))
* **deps:** pin rust crate notify-debouncer-full to =0.7.0 ([#207](https://github.com/bojanrajkovic/atc/issues/207)) ([5a44c4c](https://github.com/bojanrajkovic/atc/commit/5a44c4cb759766ccab77061081a607ed3798b02d))
* **deps:** update rust crate axum to v0.8.9 ([#123](https://github.com/bojanrajkovic/atc/issues/123)) ([cf94dfd](https://github.com/bojanrajkovic/atc/commit/cf94dfdd7b9903732d959660cb3e497fe71eca58))
* **deps:** update rust crate const-hex to v1.19.0 ([#142](https://github.com/bojanrajkovic/atc/issues/142)) ([ba0c3c9](https://github.com/bojanrajkovic/atc/commit/ba0c3c9dfcb79f5bbccdd11dde9b1dbeea3cd204))
* **deps:** update rust crate const-hex to v1.19.1 ([#285](https://github.com/bojanrajkovic/atc/issues/285)) ([e7dd727](https://github.com/bojanrajkovic/atc/commit/e7dd727e8815b66e15835bf9b51eb9b6205045ee))
* **deps:** update rust crate http to v1.4.1 ([#290](https://github.com/bojanrajkovic/atc/issues/290)) ([5b484cf](https://github.com/bojanrajkovic/atc/commit/5b484cf6ec85940173de2973b8eebe04516016af))
* **deps:** update rust crate metrics to v0.24.5 ([#124](https://github.com/bojanrajkovic/atc/issues/124)) ([953ee9c](https://github.com/bojanrajkovic/atc/commit/953ee9cabeb09521a6f003d5a17a6628dafdf2c0))
* **deps:** update rust crate reqwest to v0.13.3 ([#125](https://github.com/bojanrajkovic/atc/issues/125)) ([d215bb8](https://github.com/bojanrajkovic/atc/commit/d215bb8d540c8c106147258de7d71c722fce0c35))
* **deps:** update rust crate reqwest to v0.13.4 ([#288](https://github.com/bojanrajkovic/atc/issues/288)) ([635c6bc](https://github.com/bojanrajkovic/atc/commit/635c6bc83349aec2df896b5dcf18fcab19ff4514))
* **deps:** update rust crate serde_json to v1.0.150 ([#276](https://github.com/bojanrajkovic/atc/issues/276)) ([6702bd8](https://github.com/bojanrajkovic/atc/commit/6702bd8b6cbd873086a23899bbd647a68b750d1c))
* **deps:** update rust crate tokio to v1.52.3 ([#146](https://github.com/bojanrajkovic/atc/issues/146)) ([648da65](https://github.com/bojanrajkovic/atc/commit/648da65067aba9888cd8899fd7518f1ef47259e5))
* **deps:** update rust crate tower-http to v0.6.10 ([#126](https://github.com/bojanrajkovic/atc/issues/126)) ([d9373c3](https://github.com/bojanrajkovic/atc/commit/d9373c37a8507fc4c55fecd72b1ecbde0ef387d8))
* **deps:** update rust crate uuid to v1.23.2 ([#318](https://github.com/bojanrajkovic/atc/issues/318)) ([4c7913b](https://github.com/bojanrajkovic/atc/commit/4c7913be4b1d72c89cab6e6b91887c647963386e))
* **metrics:** source atc_build_info version from VERGEN_GIT_DESCRIBE ([96abae4](https://github.com/bojanrajkovic/atc/commit/96abae495b90cfa35494428034f2d49b37fa135c))
* **server:** support HTTPS OTLP endpoints and honor spec path semantics ([#93](https://github.com/bojanrajkovic/atc/issues/93)) ([be9960f](https://github.com/bojanrajkovic/atc/commit/be9960f2925244c79a7767e6807e0bc1473da661))
* **state-machine:** allow Queued → Completed job transition for GitHub cancellation path ([#144](https://github.com/bojanrajkovic/atc/issues/144)) ([fa78e48](https://github.com/bojanrajkovic/atc/commit/fa78e489206816a6ddda6a21ed93ac86a82f8dea))


### Performance Improvements

* **atc-server:** consolidate integration test files into a single binary ([#83](https://github.com/bojanrajkovic/atc/issues/83)) ([3a54d44](https://github.com/bojanrajkovic/atc/commit/3a54d443672abcf4a6e22095de737bcc0989230c))
