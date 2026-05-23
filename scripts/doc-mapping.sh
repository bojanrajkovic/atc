#!/usr/bin/env bash
# doc-mapping.sh — Maps source file paths to their canonical architecture docs.
#
# Used by the doc-staleness enforcement chain:
#   - Pre-push: check-docs-lefthook.sh blocking gate
#   - (Future) editor-time / commit-time advisories from Claude Code hooks
#
# Usage: source this file, then call get_docs_for_file <path>
#
# Returns one or more architecture doc paths on stdout (one per line), or
# empty output if no mapping. Files that legitimately span two architecture
# docs (e.g., a module that owns a contract documented in one place AND emits
# metrics documented in another) MUST list both — the gate dedupes the union
# across changed files and demands every listed doc was also touched.

get_docs_for_file() {
    local file="$1"

    case "$file" in
        backend/crates/atc-core/src/*)
            echo "docs/architecture/backend-server.md"
            return
            ;;
        backend/crates/atc-github/src/*)
            echo "docs/architecture/backend-server.md"
            return
            ;;
        backend/crates/atc-server/src/metrics.rs)
            # Pure metrics-surface module: registration, recorder install,
            # bucket overrides. Owned by metrics.md.
            echo "docs/architecture/metrics.md"
            return
            ;;
        backend/crates/atc-server/build.rs)
            # build.rs feeds VERGEN_* env vars consumed only by atc_build_info.
            echo "docs/architecture/metrics.md"
            return
            ;;
        backend/crates/atc-server/src/config_watcher.rs)
            # Straddler: the watcher's behavior (file detection, debounce,
            # K8s ConfigMap atomic swap, narrow-schema reload, scalar drift
            # warn-log, shutdown integration) lives in backend-server.md;
            # the watcher's emitted metrics (atc_config_reload_total,
            # atc_config_runner_pools) live in metrics.md. Both must update.
            # Listed BEFORE the atc-server/src/* catch-all because `case`
            # matching is first-match.
            echo "docs/architecture/backend-server.md"
            echo "docs/architecture/metrics.md"
            return
            ;;
        backend/crates/atc-store-pg/src/metrics.rs|backend/crates/atc-store-pg/src/listener.rs)
            # Straddlers (post-#169 phase 2 location): PgMetrics + listener carry
            # both the PersistentStore drain-pipeline contract and the metric
            # emission surface. Both docs must update.
            # Listed BEFORE the atc-store-pg/src/* catch-all because `case`
            # matching is first-match — keep more specific patterns above the
            # general one (Tuvok C3 § 7 Rule 4).
            echo "docs/architecture/backend-server.md"
            echo "docs/architecture/metrics.md"
            return
            ;;
        backend/crates/atc-store-pg/src/*|backend/crates/atc-store-pg/migrations/*)
            # Catch-all for the PG store crate (introduced in issue #169 phase 2).
            # listener.rs and metrics.rs are straddlers above; everything else
            # (store/, db.rs, reads.rs, invariants.rs, plus migrations/) is owned
            # solely by backend-server.md.
            echo "docs/architecture/backend-server.md"
            return
            ;;
        backend/crates/atc-store-mem/src/*|backend/crates/atc-wire/src/*|backend/crates/atc-persist/src/*)
            # The in-memory store, the wire-types crate, and the trait crate
            # (all introduced in issue #169 phases 1–3) carry only backend-server
            # contracts; they emit no metrics directly.
            echo "docs/architecture/backend-server.md"
            return
            ;;
        backend/crates/atc-server/src/otel.rs)
            # OTel SDK init: tracer/meter providers, sampler, propagator,
            # exponential-histogram view. Trace authoring contract + sampler
            # surface live in backend-server.md (§ Tracing); metric authoring
            # contract + histogram aggregation choice live in metrics.md.
            # Both docs must be updated when this file changes.
            echo "docs/architecture/backend-server.md"
            echo "docs/architecture/metrics.md"
            return
            ;;
        backend/crates/atc-server/src/*)
            echo "docs/architecture/backend-server.md"
            return
            ;;
        frontend/src/*)
            echo "docs/architecture/frontend-app.md"
            return
            ;;
        frontend/vite.config.ts|frontend/svelte.config.js|frontend/biome.json|frontend/eslint.config.mjs|frontend/.prettierrc|frontend/vitest.config.ts|frontend/playwright.config.ts)
            echo "docs/architecture/frontend-app.md"
            return
            ;;
        .github/workflows/release-please.yml|.github/workflows/release.yml)
            echo "docs/architecture/release-pipeline.md"
            return
            ;;
        Dockerfile|.dockerignore)
            echo "docs/architecture/release-pipeline.md"
            return
            ;;
        release-please-config.json|.release-please-manifest.json|deploy/helm/cr.yaml)
            echo "docs/architecture/release-pipeline.md"
            return
            ;;
        deploy/helm/atc/*|deploy/helm/atc/templates/*|deploy/helm/atc/templates/tests/*|deploy/helm/atc/tests/*)
            echo "docs/architecture/deployment.md"
            return
            ;;
        .github/workflows/*)
            echo "docs/architecture/ci-pipeline.md"
            return
            ;;
        scripts/helm-kubeconform*.sh|scripts/check-docs-lefthook.sh|scripts/verify-workflow-security.sh)
            echo "docs/architecture/ci-pipeline.md"
            return
            ;;
        renovate.json)
            echo "docs/architecture/ci-pipeline.md"
            return
            ;;
    esac

    # No mapping found
}
