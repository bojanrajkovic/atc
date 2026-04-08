#!/usr/bin/env bash
# doc-mapping.sh — Maps source file paths to their canonical architecture docs.
#
# Used by three layers of the documentation enforcement chain:
# 1. Editor-time: Claude Code PostToolUse hook
# 2. Commit-time: Claude Code hook advisory
# 3. Pre-push: check-docs-lefthook.sh blocking gate
#
# Usage: source this file, then call get_doc_for_file <path>
#
# Returns the architecture doc path on stdout, or empty string if no mapping.

get_doc_for_file() {
    local file="$1"

    case "$file" in
        backend/crates/atc-server/src/*)
            echo "docs/architecture/backend-server.md"
            return
            ;;
        frontend/src/*)
            echo "docs/architecture/frontend-app.md"
            return
            ;;
        frontend/vite.config.ts|frontend/svelte.config.js|frontend/biome.json|frontend/eslint.config.mjs|frontend/.prettierrc)
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
        release-please-config.json|.release-please-manifest.json)
            echo "docs/architecture/release-pipeline.md"
            return
            ;;
        .github/workflows/*)
            echo "docs/architecture/ci-pipeline.md"
            return
            ;;
    esac

    # No mapping found
    echo ""
}
