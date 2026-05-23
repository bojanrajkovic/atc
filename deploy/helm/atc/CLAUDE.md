# CLAUDE.md — deploy/helm/atc

Last verified: 2026-05-23

> Canonical documentation lives in `docs/architecture/deployment.md`. This file provides domain-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Helm chart packaging ATC for Kubernetes deployment. Published via two parallel channels on tag-triggered release: OCI on `oci://ghcr.io/bojanrajkovic/charts/atc` (Sigstore-attested) and a classic HTTP Helm repo at `https://bojanrajkovic.github.io/atc/charts`. Defaults, render-time guards, the full operator-knob surface, and contract semantics live in `docs/architecture/deployment.md`.

## Sharp edges

**`{{- if }}` strips the YAML document separator on optional templates.** When a template has an outer `{{ if }}` guarding an entire YAML document that's concatenated into the chart's output stream, use `{{ if }}` (no leading dash) so the `---` separator on the line above isn't consumed by Helm's whitespace stripping. A leading-dash trim there produces invalid YAML when the previous template's trailing newline collides with the next document. The grafana-operator CR template is the worked example.

**ConfigMaps that need hot-reload propagation must be directory mounts, not `subPath`.** kubelet does NOT propagate ConfigMap updates through a `subPath` mount — the file at that subPath is a snapshot at pod startup. ATC's `runner-pool-config` ConfigMap mount deliberately mounts the whole ConfigMap as a directory (`mountPath: /etc/atc`, no `subPath`) so the `config_watcher` task observes operator edits. If a future template tempts you to use `subPath` for a hot-reload-bearing ConfigMap, the propagation path silently breaks.

## Commands

```bash
helm lint deploy/helm/atc                                # Lint
helm template atc deploy/helm/atc                        # Render
helm unittest deploy/helm/atc                            # helm-unittest suites
helm template atc deploy/helm/atc | kubeconform -strict  # Validate against k8s schemas
```

## Key References

- Architecture: `docs/architecture/deployment.md`
- Release pipeline: `docs/architecture/release-pipeline.md`
- CI validation: `docs/architecture/ci-pipeline.md` (helm-lint, helm-validate kubeconform sweep, helm-install kind + chart-testing)
