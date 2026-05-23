#!/usr/bin/env bash
# check-agents-symlinks.sh — Pre-push gate enforcing CLAUDE.md ⇄ AGENTS.md
# symlinks.
#
# For every CLAUDE.md in the tree, verifies that AGENTS.md in the same
# directory exists as a symlink pointing at CLAUDE.md. Blocks (exit 1) on
# missing or wrong-shaped links — keeping the two filenames in lockstep so
# tools that look for either find the same content.
#
# Called by lefthook's pre-push hook.

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$REPO_ROOT"

has_violations=false
while IFS= read -r claude; do
    dir="$(dirname "$claude")"
    agents="$dir/AGENTS.md"

    if [ ! -e "$agents" ] && [ ! -L "$agents" ]; then
        echo "ERROR: $claude has no sibling AGENTS.md"
        has_violations=true
        continue
    fi

    if [ ! -L "$agents" ]; then
        echo "ERROR: $agents exists but is not a symlink (should point to CLAUDE.md)"
        has_violations=true
        continue
    fi

    target=$(readlink "$agents")
    if [ "$target" != "CLAUDE.md" ]; then
        echo "ERROR: $agents is a symlink to '$target', expected 'CLAUDE.md'"
        has_violations=true
    fi
done < <(
    find . -type f -name 'CLAUDE.md' \
        -not -path './node_modules/*' \
        -not -path './target/*' \
        -not -path './.git/*' \
        -not -path './.claude/worktrees/*' \
        2>/dev/null
)

if [ "$has_violations" = true ]; then
    echo ""
    echo "Every CLAUDE.md must have a sibling AGENTS.md symlink:"
    echo "  cd <dir> && ln -s CLAUDE.md AGENTS.md"
    exit 1
fi

exit 0
