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

    # Phase 1: No mappings yet. Add entries as architecture docs are created.
    # Example mapping (uncomment and adapt when adding first architecture doc):
    #
    # case "$file" in
    #     backend/src/*)
    #         echo "docs/architecture/backend-api.md"
    #         return
    #         ;;
    #     frontend/src/*)
    #         echo "docs/architecture/frontend-ui.md"
    #         return
    #         ;;
    # esac

    # No mapping found
    echo ""
}
