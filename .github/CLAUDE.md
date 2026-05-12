# CLAUDE.md — .github

Last verified: 2026-05-11

> Canonical documentation lives in `docs/architecture/ci-pipeline.md` and `docs/architecture/release-pipeline.md`. This file provides domain-specific guidance for agents working here. Do not duplicate content from the architecture docs.

## Purpose

GitHub Actions workflows for CI, security linting, versioning, and release artifact production.

## Workflows

| File | Trigger | Role |
|------|---------|------|
| `workflows/ci.yml` | PR + push to main | Lint, type-check, test, build (path-filtered on PRs, full matrix on main) |
| `workflows/zizmor.yml` | Workflow file changes | Security linter for Actions workflow files |
| `workflows/release-please.yml` | Push to main | Conventional commit analysis, version bumps, release PR management |
| `workflows/release.yml` | Tag `v*` | Multi-arch binary builds, Docker image, Helm chart, Sigstore attestation |

## Contracts

- **Path filtering on PRs:** Only affected stacks run (backend, frontend, helm). Main always runs full matrix.
- **Helm validate sweep:** 2 Kubernetes versions × 9 values files (`defaults`, `ingress`, `gateway`, `multi-replica`, `otel`, `existing-secret-listener`, `pdb`, `networkpolicy`, `autoscaling`) run sequentially inside a single `helm-validate` runner via `scripts/helm-kubeconform.sh`. Emits a Markdown pass/fail table to `$GITHUB_STEP_SUMMARY`.
- **Linked versions:** All crates + frontend version in lockstep via release-please.

## Key References

- CI architecture: `docs/architecture/ci-pipeline.md`
- Release architecture: `docs/architecture/release-pipeline.md`
- Workflow security: zizmor (https://woodruffw.github.io/zizmor/)
