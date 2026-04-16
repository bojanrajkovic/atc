#!/usr/bin/env bash
# check-docs-lefthook.sh — Pre-push doc-staleness gate.
#
# Performs a branch-scoped diff against origin/main and blocks (exit 1) if
# source files were modified but their architecture doc was not.
#
# Called by lefthook's pre-push hook.
#
# Exit codes:
#   0 — all modified source files have up-to-date docs (or no mappings exist)
#   1 — at least one source file was modified without updating its architecture doc

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck source=./doc-mapping.sh
source "${SCRIPT_DIR}/doc-mapping.sh"

# Get files changed in this branch compared to origin/main
MERGE_BASE=$(git merge-base HEAD origin/main 2>/dev/null || echo "")

if [ -z "$MERGE_BASE" ]; then
    # No merge base found — likely on main or initial commit. Nothing to check.
    exit 0
fi

CHANGED_FILES=$(git diff --name-only "$MERGE_BASE"..HEAD)

if [ -z "$CHANGED_FILES" ]; then
    # No changes — nothing to check
    exit 0
fi

# Collect unique docs that need updating (one per line, deduplicated)
docs_needed=""
while IFS= read -r file; do
    doc=$(get_doc_for_file "$file")
    if [ -n "$doc" ]; then
        docs_needed=$(printf '%s\n%s' "$docs_needed" "$doc" | sort -u)
    fi
done <<< "$CHANGED_FILES"

# Trim leading blank line from printf
docs_needed=$(echo "$docs_needed" | sed '/^$/d')

# If no mappings matched, nothing to enforce
if [ -z "$docs_needed" ]; then
    exit 0
fi

# Check which required docs were also modified
has_violations=false
while IFS= read -r doc; do
    if ! grep -qxF "$doc" <<< "$CHANGED_FILES"; then
        echo "ERROR: Source files mapped to '$doc' were modified, but '$doc' was not updated."
        has_violations=true
    fi
done <<< "$docs_needed"

if [ "$has_violations" = true ]; then
    echo ""
    echo "Update the listed architecture docs or modify scripts/doc-mapping.sh if the mapping is outdated."
    exit 1
fi

exit 0
