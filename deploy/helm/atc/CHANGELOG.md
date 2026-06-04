# Changelog

## [0.2.0](https://github.com/bojanrajkovic/atc/compare/v0.1.0...v0.2.0) (2026-06-04)


### Features

* **ci:** chart-testing on ephemeral kind cluster ([#90](https://github.com/bojanrajkovic/atc/issues/90)) ([d889680](https://github.com/bojanrajkovic/atc/commit/d88968081a8ca2cb82f8ceb72ca43dd0ad609757))
* **config:** hot-reload runner_pools without restart ([#172](https://github.com/bojanrajkovic/atc/issues/172)) ([#205](https://github.com/bojanrajkovic/atc/issues/205)) ([3576d17](https://github.com/bojanrajkovic/atc/commit/3576d17cb389cc6e7f8073b21f2f25232453184d))
* configurable runner pool capacity per label set ([#177](https://github.com/bojanrajkovic/atc/issues/177)) ([5cba11e](https://github.com/bojanrajkovic/atc/commit/5cba11efaa298b2aa5e04452774a2539ceb5a36e))
* helm chart, metrics, and release publishing for ATC ([#14](https://github.com/bojanrajkovic/atc/issues/14)) ([503e96f](https://github.com/bojanrajkovic/atc/commit/503e96f6b8a3f211733bcc0c78460d9f0bdf701f))
* **helm:** add HorizontalPodAutoscaler template ([#89](https://github.com/bojanrajkovic/atc/issues/89)) ([1a25ed7](https://github.com/bojanrajkovic/atc/commit/1a25ed76acf27fcc7859420890928ce117bec08a)), closes [#8](https://github.com/bojanrajkovic/atc/issues/8)
* **helm:** add NetworkPolicy template ([#88](https://github.com/bojanrajkovic/atc/issues/88)) ([1603712](https://github.com/bojanrajkovic/atc/commit/160371242746585fcf29fdf9b8ae01c5a5d4c3f0))
* **helm:** add PodDisruptionBudget template ([#87](https://github.com/bojanrajkovic/atc/issues/87)) ([f8a151d](https://github.com/bojanrajkovic/atc/commit/f8a151d08c0c2a5be741228ae7c6df3388f0e815)), closes [#9](https://github.com/bojanrajkovic/atc/issues/9)
* **helm:** bundle modernized Grafana dashboard with sidecar + operator discovery ([#224](https://github.com/bojanrajkovic/atc/issues/224)) ([78fc783](https://github.com/bojanrajkovic/atc/commit/78fc783b6bbbd7f2608a9d6e1dc05020088444ad))
* **helm:** gate multi-replica on postgres, remove sqlite/persistence ([#7](https://github.com/bojanrajkovic/atc/issues/7)) ([#57](https://github.com/bojanrajkovic/atc/issues/57)) ([6b7d0e4](https://github.com/bojanrajkovic/atc/commit/6b7d0e4b36b8814fc2568f3ee323aa2895ce9827))
* **helm:** graceful shutdown deploy surface — preStop hook and readiness probe coordination ([#85](https://github.com/bojanrajkovic/atc/issues/85)) ([6f00d19](https://github.com/bojanrajkovic/atc/commit/6f00d19435c23b3a27134d189b89f0e6ddbcbae9))
* **helm:** pod anti-affinity defaults for multi-replica ([#86](https://github.com/bojanrajkovic/atc/issues/86)) ([4ecd82a](https://github.com/bojanrajkovic/atc/commit/4ecd82a3f6f16582dd0783b8e296c66bd0e58545)), closes [#10](https://github.com/bojanrajkovic/atc/issues/10)
* **helm:** restructure existingSecret into per-credential blocks ([#242](https://github.com/bojanrajkovic/atc/issues/242)) ([4fbf138](https://github.com/bojanrajkovic/atc/commit/4fbf1385841fe3fcdb994b37ee6334f5f93b41d2))
* observability pass ([#245](https://github.com/bojanrajkovic/atc/issues/245)) ([ebf6118](https://github.com/bojanrajkovic/atc/commit/ebf611851e76f73768a3fea04d5f14854751854c))
* **release:** publish chart via chart-releaser to GitHub Pages ([#92](https://github.com/bojanrajkovic/atc/issues/92)) ([7172a46](https://github.com/bojanrajkovic/atc/commit/7172a463c96d640c4457f007734773b6cb88ac29))
* **runner-pools:** allow unbounded capacity via `capacity: null` ([#206](https://github.com/bojanrajkovic/atc/issues/206)) ([7d45fe0](https://github.com/bojanrajkovic/atc/commit/7d45fe0cc5df4a63d17e077aae03efe91daf8e61))
* **server:** add LISTEN/NOTIFY end-to-end with listener fetch-and-log stub ([#52](https://github.com/bojanrajkovic/atc/issues/52)) ([340f675](https://github.com/bojanrajkovic/atc/commit/340f6759f347e9a05dfd369ea9fc15524d3cf8a3))
* **server:** introduce OpenTelemetry instrumentation (tracing and metrics) ([#91](https://github.com/bojanrajkovic/atc/issues/91)) ([feb2858](https://github.com/bojanrajkovic/atc/commit/feb285880c8a253cf17946484e92b4421fe16439))
* **server:** outbox retention — heartbeat + sweep + metrics ([#67](https://github.com/bojanrajkovic/atc/issues/67)) ([#192](https://github.com/bojanrajkovic/atc/issues/192)) ([202b1dd](https://github.com/bojanrajkovic/atc/commit/202b1dd9b058e1f39dafdcc60dcb500d5f8e3fe6))
* wire server endpoints — webhook ingestion, WebSocket streaming, REST state snapshot ([#21](https://github.com/bojanrajkovic/atc/issues/21)) ([4cb6e55](https://github.com/bojanrajkovic/atc/commit/4cb6e558ee91d1881bf719c19477fc12d45c17bf))


### Bug Fixes

* **helm:** correct dashboard queries for OTLP→Prometheus label shapes ([d53e63e](https://github.com/bojanrajkovic/atc/commit/d53e63e96a46b23706bf3af0c5e0650d9aaa77b0))

## Changelog
