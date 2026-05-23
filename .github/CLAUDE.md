# CLAUDE.md — .github

Last verified: 2026-05-23

> Canonical documentation lives in `docs/architecture/ci-pipeline.md` and `docs/architecture/release-pipeline.md`. This file provides domain-specific guidance for agents working here. Do not duplicate content from the architecture docs.

## Purpose

GitHub Actions workflows for CI, security linting, versioning, and release artifact production. The workflow files in `workflows/` are individually self-documenting via their `name:` and trigger blocks; the arch docs cover the contracts (path filtering, helm validate sweep, linked versions, multi-arch image build, Sigstore attestation).

## Key References

- CI architecture: `docs/architecture/ci-pipeline.md`
- Release architecture: `docs/architecture/release-pipeline.md`
- Workflow security: zizmor (https://woodruffw.github.io/zizmor/)
