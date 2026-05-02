#!/usr/bin/env bash
# Pre-commit gate: block new `as any` casts and `biome-ignore lint/suspicious/noExplicitAny`
# suppressions in frontend source.
#
# Why: biome's `noExplicitAny` rule is set to "warn" (not "error") for historical
# reasons — promoting it would require retrofitting existing legitimate sites.
# Pre-2026-05-02, prose-only memory rules ("don't use `as any`") have not
# reliably prevented agents from regressing on this. This hook is the
# mechanical-enforcement layer.
#
# Scope: only checks ADDED lines in staged frontend changes. Pre-existing
# `as any` sites (src/main.ts window-bridge, e2e/lib/ws-mock.ts test bridge,
# vendored shadcn-svelte components) remain.
#
# To override (rare, requires real justification):
#   1. Add the file to the ALLOWLIST below with reviewer approval, OR
#   2. Run `git commit --no-verify` and explain in the PR description.

set -euo pipefail

# Files where `as any` is structurally legitimate (test/dev bridges, generated code).
ALLOWLIST=(
  "frontend/src/main.ts"
  "frontend/src/vite-env.d.ts"
  "frontend/e2e/lib/ws-mock.ts"
)

# Build a regex that matches any allowlisted path so we can exclude their diffs.
ALLOW_REGEX=$(printf "|%s" "${ALLOWLIST[@]}")
ALLOW_REGEX="${ALLOW_REGEX:1}"  # trim leading |

# Get staged diff for frontend .ts and .svelte files, excluding allowlisted paths.
# `--diff-filter=AM` covers Adds + Modifies; pure deletes don't count.
diff_output=$(
  git diff --cached --diff-filter=AM --unified=0 -- \
    ':(glob)frontend/src/**/*.ts' \
    ':(glob)frontend/src/**/*.svelte' \
    ':(glob)frontend/e2e/**/*.ts' \
    ':(glob)frontend/e2e/**/*.svelte' \
  | awk -v allow="$ALLOW_REGEX" '
      /^diff --git/ { file = $0; in_allow = (file ~ allow); next }
      in_allow { next }
      /^\+\+\+/ || /^---/ { next }
      /^\+/ { print file "\n" $0 }
    '
)

# Find ADDED lines containing forbidden patterns.
violations=$(printf '%s' "$diff_output" | grep -nE '\bas any\b|biome-ignore lint/suspicious/noExplicitAny' || true)

if [ -n "$violations" ]; then
  echo "❌ Blocked: new \`as any\` cast or \`biome-ignore\` of \`noExplicitAny\` detected"
  echo
  echo "$violations"
  echo
  echo "What to do instead:"
  echo "  1. The literal often already typechecks — try removing the cast and running"
  echo "     \`pnpm check\` to see if it succeeds."
  echo "  2. Widen the parameter type with a discriminated union or proper generic."
  echo "  3. Use a typed factory (see frontend/src/lib/test-utils/factories.ts patterns)."
  echo "  4. If the cast is genuinely required (rare), add the file to the ALLOWLIST"
  echo "     in scripts/check-no-explicit-any.sh with reviewer approval."
  echo
  echo "If you must bypass for an exceptional case, run \`git commit --no-verify\`"
  echo "and justify the bypass in the PR description."
  exit 1
fi

exit 0
