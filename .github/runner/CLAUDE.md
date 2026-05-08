# CLAUDE.md — .github/runner

Last verified: 2026-05-08

> Canonical documentation lives in `docs/architecture/ci-pipeline.md` (§ "Self-hosted ARC runner image"). This file provides domain-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Custom ARC (Actions Runner Controller) runner image for the homelab Kubernetes cluster. Pre-bakes the apt packages the ATC workflows need, eliminating per-job runtime install overhead. The same workflow's runtime apt-install steps remain in place as a portability safety net (no-ops when the image already has the packages).

## Files

| File | Role |
|------|------|
| `Dockerfile` | Image definition, `FROM ghcr.io/actions/actions-runner:<version>` + apt deps + Playwright deps via `npx playwright install-deps chromium` |
| `.dockerignore` | Build context allowlist (just the Dockerfile) |
| `k8s/image-updater.yaml` | CronJob that resolves `:latest` to a digest and patches the AutoscalingRunnerSet weekly |

The build workflow lives at `.github/workflows/runner-image.yml` (in the workflows directory because GitHub Actions discovers workflows there only).

## Contracts

- **Playwright version comes from `frontend/package.json`.** The build workflow extracts `devDependencies["@playwright/test"]` and passes it as a `PLAYWRIGHT_VERSION` build arg. Self-maintaining: bumping Playwright in the project triggers a runner-image rebuild on the next push to `main` (path-filtered on `frontend/package.json`).
- **Image is published to `ghcr.io/<owner>/atc-runner`** with `latest` and `sha-<commit>` tags. The package must be public so the K8s CronJob's anonymous GHCR token exchange works.
- **Weekly rebuild Monday 06:00 UTC**, K8s update Monday 08:00 UTC. Two-hour gap leaves room for build slowness without overlapping the digest-resolve.
- **Base image is pinned** to a specific `ghcr.io/actions/actions-runner` version (not `:latest`). Bump intentionally alongside upstream releases.

## Key References

- Architecture doc: `docs/architecture/ci-pipeline.md` (§ "Self-hosted ARC runner image")
- Build workflow: `.github/workflows/runner-image.yml`
- ARC scale set deployment: external (homelab Kubernetes manifests, not in this repo)
