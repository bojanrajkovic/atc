# Custom ARC runner image for the ATC repo. Pre-bakes the apt packages
# the workflows would otherwise install at runtime. See
# docs/architecture/ci-pipeline.md → "Self-hosted ARC runner image" for
# the rationale.
#
# Base image is pinned to a specific runner version for reproducibility;
# bump intentionally alongside the upstream actions/runner release. The
# weekly rebuild workflow picks up Ubuntu 24.04 base image security
# updates and Playwright dep changes without touching this pin.
FROM ghcr.io/actions/actions-runner:2.333.0@sha256:1ad983536759ceec39ed75a2c8f007ca8c37b66eee35ed86f13623b29a4db97d

# Project-side Playwright version, passed by the build workflow which
# extracts it from frontend/package.json. Keeping the pin in
# package.json (the project's source of truth) and propagating to the
# image build avoids the drift that would happen with a hardcoded
# version in this Dockerfile.
ARG PLAYWRIGHT_VERSION

USER root

# Stable build/runtime deps that don't change with Playwright versions.
# Sourced from the workflow's runtime apt installs:
#   - ci.yml backend "Install build dependencies": Rust crate compile
#     (build-essential, cmake, pkg-config, perl), linker (mold)
#   - release.yml build-binaries "Install musl + cross toolchain":
#     musl-tools, gcc-aarch64-linux-gnu (for arm64 cross-compile)
#   - ci.yml frontend / release.yml build-frontend libatomic1: Node 25
#     prebuilt binary linkage
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        cmake \
        pkg-config \
        perl \
        mold \
        musl-tools \
        gcc-aarch64-linux-gnu \
        libatomic1 \
    && rm -rf /var/lib/apt/lists/*

# Playwright chromium runtime libs + fonts. We let Playwright tell us
# what it needs by running `playwright install-deps chromium` at image
# build time — the canonical list lives in
# `playwright-core/lib/server/registry/nativeDeps.js` and evolves with
# Playwright releases. Self-maintaining: rebuilding with a different
# PLAYWRIGHT_VERSION pulls the right list automatically. The browser
# binary itself is fetched per-workflow via `playwright install
# chromium`; only the OS deps are baked here.
#
# Uses the runner image's bundled Node 24 via an inline PATH override on
# the RUN command — NOT an `ENV PATH=...` directive — so the override is
# scoped to this build step only. Persisting Node 24 on PATH at image
# runtime would shadow the version that workflow jobs install via
# setup-node/mise, causing version-drift surprises.
RUN apt-get update \
    && PATH="/home/runner/externals/node24/bin:${PATH}" \
       npx --yes playwright@${PLAYWRIGHT_VERSION} install-deps chromium \
    && rm -rf /var/lib/apt/lists/*

USER runner
