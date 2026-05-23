#!/usr/bin/env bash
# check-docs.sh — Pre-push doc-staleness gate.
#
# Reads scripts/doc-mapping.yaml to derive the set of architecture docs that
# must be updated when source files change. Blocks (exit 1) if any required
# doc was NOT also touched in the branch's diff against origin/main.
#
# Called by lefthook's pre-push hook.
#
# Exit codes:
#   0 — all modified source files have up-to-date docs (or no mappings exist)
#   1 — at least one source file was modified without updating its arch doc

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MAPPING="${SCRIPT_DIR}/doc-mapping.yaml"

if [ ! -f "$MAPPING" ]; then
    echo "ERROR: $MAPPING not found" >&2
    exit 1
fi

# Resolve yq through mise so contributors don't need a separate yq install.
YQ() {
    mise exec -- yq "$@"
}

# Get files changed in this branch compared to origin/main
MERGE_BASE=$(git merge-base HEAD origin/main 2>/dev/null || echo "")

if [ -z "$MERGE_BASE" ]; then
    # No merge base — likely on main or initial commit. Nothing to check.
    exit 0
fi

CHANGED_FILES=$(git diff --name-only "$MERGE_BASE"..HEAD)

if [ -z "$CHANGED_FILES" ]; then
    exit 0
fi

# Flatten the YAML to a tab-separated lookup table once at startup:
#   group_index<TAB>pattern<TAB>doc
# One line per (pattern, doc) pair, preserving the YAML's top-to-bottom group
# order so first-match-wins semantics work in the loop below.
FLAT=$(YQ -r '
  .mappings | to_entries | .[] as $entry
  | $entry.value.patterns[] as $pat
  | $entry.value.docs[] as $doc
  | "\($entry.key)\t\($pat)\t\($doc)"
' "$MAPPING") || {
    echo "ERROR: failed to parse $MAPPING — check YAML syntax" >&2
    exit 1
}

# For each changed file, find the first matching group (first-match-wins) and
# emit every doc that group lists. Files matching no pattern contribute
# nothing.
get_docs_for_file() {
    local file="$1"
    local matched_group=""

    while IFS=$'\t' read -r g pat _doc; do
        # shellcheck disable=SC2053
        if [[ "$file" == $pat ]]; then
            matched_group="$g"
            break
        fi
    done <<< "$FLAT"

    if [ -z "$matched_group" ]; then
        return
    fi

    while IFS=$'\t' read -r g _pat doc; do
        if [ "$g" = "$matched_group" ]; then
            echo "$doc"
        fi
    done <<< "$FLAT"
}

# Collect unique docs that need updating across all changed files.
docs_needed=""
while IFS= read -r file; do
    docs=$(get_docs_for_file "$file")
    if [ -n "$docs" ]; then
        docs_needed=$(printf '%s\n%s' "$docs_needed" "$docs" | sort -u | sed '/^$/d')
    fi
done <<< "$CHANGED_FILES"

if [ -z "$docs_needed" ]; then
    exit 0
fi

# Block on any required doc that wasn't itself in the change set.
has_violations=false
while IFS= read -r doc; do
    if ! grep -qxF "$doc" <<< "$CHANGED_FILES"; then
        echo "ERROR: Source files mapped to '$doc' were modified, but '$doc' was not updated."
        has_violations=true
    fi
done <<< "$docs_needed"

if [ "$has_violations" = true ]; then
    echo ""
    echo "Update the listed architecture docs or modify scripts/doc-mapping.yaml if the mapping is outdated."
    exit 1
fi

exit 0
