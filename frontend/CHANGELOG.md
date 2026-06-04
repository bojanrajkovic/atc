# Changelog

## [0.2.0](https://github.com/bojanrajkovic/atc/compare/v0.1.0...v0.2.0) (2026-06-04)


### Features

* add repo skeleton with Rust backend and Svelte frontend ([19dc156](https://github.com/bojanrajkovic/atc/commit/19dc156d4ad3959bb232ac245430ab6eb7664291))
* align the state cursor contract and derive runner pools in the frontend ([4d01b1b](https://github.com/bojanrajkovic/atc/commit/4d01b1bc7928a2eebd8295ad14e58485a471d9fa))
* broadcast live runner pool stats and disambiguate runner bar labels ([#34](https://github.com/bojanrajkovic/atc/issues/34)) ([8e0f2af](https://github.com/bojanrajkovic/atc/commit/8e0f2afa9ece4e1c02fb6074f53c94e99e9fcd55))
* **config:** hot-reload runner_pools without restart ([#172](https://github.com/bojanrajkovic/atc/issues/172)) ([#205](https://github.com/bojanrajkovic/atc/issues/205)) ([3576d17](https://github.com/bojanrajkovic/atc/commit/3576d17cb389cc6e7f8073b21f2f25232453184d))
* configurable runner pool capacity per label set ([#177](https://github.com/bojanrajkovic/atc/issues/177)) ([5cba11e](https://github.com/bojanrajkovic/atc/commit/5cba11efaa298b2aa5e04452774a2539ceb5a36e))
* **frontend:** add Cmd+K palette, detail panel, hover-peek, and pool filter ([#41](https://github.com/bojanrajkovic/atc/issues/41)) ([abb99a6](https://github.com/bojanrajkovic/atc/commit/abb99a6a7a487577ed099090ea31af558d27c02a))
* **frontend:** cap ConnectionManager reconnects and surface click-to-retry ([2471f5d](https://github.com/bojanrajkovic/atc/commit/2471f5df8a23fa8db41fb9b5896878d3c3d588da))
* **frontend:** polish, responsive layout, ARIA live region, and performance verification ([#45](https://github.com/bojanrajkovic/atc/issues/45)) ([288e263](https://github.com/bojanrajkovic/atc/commit/288e2634a72a6033bf7ffaababeb1f6bd1f9aa93))
* **frontend:** roving-tabindex keyboard navigation for kanban grid ([#43](https://github.com/bojanrajkovic/atc/issues/43)) ([58c67f5](https://github.com/bojanrajkovic/atc/commit/58c67f58d20558f77cc64581603de04c05845a7b))
* **frontend:** surface config-reload errors via admin alert banner ([#230](https://github.com/bojanrajkovic/atc/issues/230)) ([8767484](https://github.com/bojanrajkovic/atc/commit/87674843ba09bbb05d5c0c1a7813f26f1fad3423))
* **frontend:** URL-based deep linking for selected run ([#241](https://github.com/bojanrajkovic/atc/issues/241)) ([482f4f7](https://github.com/bojanrajkovic/atc/commit/482f4f7e5b390a382819ad427b91e3311bd65fcb))
* handle GitHub re-runs with attempt-aware run and job state ([#303](https://github.com/bojanrajkovic/atc/issues/303)) ([c7eb703](https://github.com/bojanrajkovic/atc/commit/c7eb703e8a856d107d9584fb85dbc31db46c1e0e))
* implement app shell with top bar, runner indicators, and settings popover ([#23](https://github.com/bojanrajkovic/atc/issues/23)) ([d75fed9](https://github.com/bojanrajkovic/atc/commit/d75fed9bd0ab5b4d7ea8a9fe901c579d4b79c59e))
* implement frontend foundation — type generation, design system, stores, WebSocket client, E2E tests ([2a7e416](https://github.com/bojanrajkovic/atc/commit/2a7e41675883dedeeac5e2016d7e4bb89a62f50d))
* implement frontend foundation — type generation, design system, stores, WebSocket client, E2E tests ([#22](https://github.com/bojanrajkovic/atc/issues/22)) ([2a7e416](https://github.com/bojanrajkovic/atc/commit/2a7e41675883dedeeac5e2016d7e4bb89a62f50d))
* implement kanban board with three-column layout, card animations, and E2E coverage ([#25](https://github.com/bojanrajkovic/atc/issues/25)) ([c68b6f8](https://github.com/bojanrajkovic/atc/commit/c68b6f8b4ec37bed34350f71f2001164cd0bbd40))
* **persist:** extract atc-wire and atc-persist crates, rename SeqEvent to CommittedEvent ([#169](https://github.com/bojanrajkovic/atc/issues/169)) ([#198](https://github.com/bojanrajkovic/atc/issues/198)) ([1e0edd9](https://github.com/bojanrajkovic/atc/commit/1e0edd9a27a8962b9395b8d5776668d3e0da4342))
* protocol version handshake and GoingAway envelope ([#227](https://github.com/bojanrajkovic/atc/issues/227)) ([87c1fd6](https://github.com/bojanrajkovic/atc/commit/87c1fd6f22cd1a67ba018c3bcf0eb5571e9c925d))
* release pipeline (release-please, multi-arch container, attestation) ([#4](https://github.com/bojanrajkovic/atc/issues/4)) ([09bec02](https://github.com/bojanrajkovic/atc/commit/09bec02baefccf2e99878592cbbc851ff5162fa5))
* **runner-pools:** allow unbounded capacity via `capacity: null` ([#206](https://github.com/bojanrajkovic/atc/issues/206)) ([7d45fe0](https://github.com/bojanrajkovic/atc/commit/7d45fe0cc5df4a63d17e077aae03efe91daf8e61))
* wire server endpoints — webhook ingestion, WebSocket streaming, REST state snapshot ([#21](https://github.com/bojanrajkovic/atc/issues/21)) ([4cb6e55](https://github.com/bojanrajkovic/atc/commit/4cb6e558ee91d1881bf719c19477fc12d45c17bf))


### Bug Fixes

* **deps:** pin dependencies ([#97](https://github.com/bojanrajkovic/atc/issues/97)) ([4e8ff1c](https://github.com/bojanrajkovic/atc/commit/4e8ff1cb982864af989097dad28aed9358af2e86))
* **frontend:** swap Toggle for Switch in SettingsPopover ([#72](https://github.com/bojanrajkovic/atc/issues/72)) ([914a52b](https://github.com/bojanrajkovic/atc/commit/914a52b3e7ed545e7011d828735c4699c6b0f951))
* **runners:** exclude orphaned Queued jobs from completed runs in pool stats ([cb2e962](https://github.com/bojanrajkovic/atc/commit/cb2e962c794ee4a1e81702faae6fd15583df0bf0))


### Performance Improvements

* **frontend:** migrate RunStore Maps to SvelteMap for per-key reactivity ([#26](https://github.com/bojanrajkovic/atc/issues/26)) ([#33](https://github.com/bojanrajkovic/atc/issues/33)) ([eb42015](https://github.com/bojanrajkovic/atc/commit/eb4201509a2d03da61d48fc1ea6de8fdd6a8342b))
* **frontend:** pace WS batch injection across rAF boundaries ([#46](https://github.com/bojanrajkovic/atc/issues/46)) ([#204](https://github.com/bojanrajkovic/atc/issues/204)) ([3041aa0](https://github.com/bojanrajkovic/atc/commit/3041aa0b979054f8eaf498da7d0ac2780602aa40))
