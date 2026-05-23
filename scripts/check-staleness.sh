#!/usr/bin/env bash
# check-staleness.sh — Heuristic staleness scan (warn-only).
#
# Greps a fixed set of stale-fact placeholder phrases across CLAUDE.md /
# AGENTS.md and ADR files. Always exits 0 — surfaces warnings without blocking
# the push. The list is opinionated and small; new patterns get added when an
# audit catches another recurring placeholder shape.
#
# Patterns scanned:
#   - "until X lands"          — usually pre-shipping caveats
#   - "until issue #N"         — same, with a tracker reference
#   - "in-tree until"          — module-relocation transitional state
#   - "for now"                — provisional behavior placeholders
#
# Called by lefthook's pre-push hook.

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$REPO_ROOT"

PATTERN='(until [^[:space:]]+ lands|until issue #[0-9]+|in-tree until|for now)'

# Collect candidate files: every CLAUDE.md / AGENTS.md in the tree (skipping
# vendor + build-artifact + worktree directories with -prune so find doesn't
# descend into the 110 GB cargo target/ tree), plus every committed ADR.
files=$(
    find . \
        \( -path ./.git \
        -o -path ./.claude/worktrees \
        -o -path ./backend/target \
        -o -path ./frontend/node_modules \
        -o -path ./node_modules \
        -o -path ./target \
        \) -prune \
        -o -type f \
        \( -name 'CLAUDE.md' -o -name 'AGENTS.md' \) \
        -print \
        2>/dev/null
    ls docs/architecture-decisions/*.md 2>/dev/null || true
)

if [ -z "$files" ]; then
    exit 0
fi

# `grep -L` would let us short-circuit, but we want the matches printed.
# Run grep, suppress its non-zero exit when nothing matches.
matches=$(echo "$files" | xargs grep -EHn -- "$PATTERN" 2>/dev/null || true)

if [ -n "$matches" ]; then
    echo ""
    echo "  ⚠️  Staleness scan: potential stale-fact phrases found"
    echo "$matches" | sed 's/^/    /'
    echo ""
    echo "    (warn-only — these may be intentional, but worth a quick look)"
fi

exit 0
