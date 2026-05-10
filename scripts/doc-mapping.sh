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
        backend/crates/atc-server/src/persist.rs|backend/crates/atc-server/src/listener.rs)
            # Straddlers: PersistentStore trait / drain pipeline contracts live
            # in backend-server.md; metric emissions documented in metrics.md.
            # Both docs must be updated when either file changes.
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
        Dockerfile|Dockerfile.release|.dockerignore|Dockerfile.release.dockerignore)
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
        .github/workflows/*|.github/runner/*|.github/runner/k8s/*|.github/actionlint.yaml)
            echo "docs/architecture/ci-pipeline.md"
            return
            ;;
    esac

    # No mapping found
}
