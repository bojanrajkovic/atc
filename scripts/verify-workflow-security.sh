#!/bin/bash
set -euo pipefail

# Workflow Security Verification Script
# Verifies AC8.1, AC8.2, AC8.3 from test-requirements.md
# AC8.1: All `uses:` references are SHA-pinned with version comments
# AC8.2: All checkouts use `persist-credentials: false`
# AC8.3: Workflow-level permissions are `{}`

failed=0
error_details=""

# Find all workflow files
workflow_files=()
while IFS= read -r file; do
    workflow_files+=("$file")
done < <(find .github/workflows -name "*.yml" -o -name "*.yaml" 2>/dev/null || true)

if [[ ${#workflow_files[@]} -eq 0 ]]; then
    echo "ERROR: No workflow files found in .github/workflows/"
    exit 1
fi

echo "Checking $(basename "$PWD"): workflow security"
echo "Found ${#workflow_files[@]} workflow file(s)"
echo

# AC8.3: Check top-level permissions: {} for each workflow file
echo "AC8.3: Checking top-level permissions are set to {} in all workflows..."
for file in "${workflow_files[@]}"; do
    # Extract top-level permissions line (should be "permissions: {}")
    if grep -q "^permissions:\s*{}\s*$" "$file"; then
        # Found it correctly
        :
    else
        # Either missing or incorrect
        if grep -q "^permissions:" "$file"; then
            echo "  FAIL: $file — 'permissions:' is not '{}'"
            error_details="${error_details}AC8.3 FAIL ($file): 'permissions:' must be '{}'\n"
            failed=1
        else
            echo "  FAIL: $file — missing top-level 'permissions:' key"
            error_details="${error_details}AC8.3 FAIL ($file): Top-level 'permissions:' not found\n"
            failed=1
        fi
    fi
done
[[ $failed -eq 0 ]] && echo "  PASS: All workflows have 'permissions: {}'"
echo

# AC8.1: Check all uses: lines are SHA-pinned with # v comment
echo "AC8.1: Checking all 'uses:' references are SHA-pinned with version comments..."
for file in "${workflow_files[@]}"; do
    # Extract all uses: lines
    uses_lines=$(grep -E "^\s*(- )?uses:" "$file" || true)
    if [[ -z "$uses_lines" ]]; then
        continue
    fi

    while IFS= read -r uses_line; do
        # Extract the reference (everything after 'uses:')
        ref=$(echo "$uses_line" | sed 's/.*uses:\s*\(.*\)\s*$/\1/')

        # Check for @[0-9a-f]{40} pattern (40-char hex SHA)
        if ! echo "$ref" | grep -qE '@[0-9a-f]{40}'; then
            echo "  FAIL: $file — uses: reference not SHA-pinned: $ref"
            error_details="${error_details}AC8.1 FAIL ($file): Not SHA-pinned: $ref\n"
            failed=1
        fi

        # Check for # v comment
        if ! echo "$ref" | grep -qE '#\s*v[0-9]'; then
            echo "  FAIL: $file — uses: reference missing '# v' comment: $ref"
            error_details="${error_details}AC8.1 FAIL ($file): Missing '# v' comment: $ref\n"
            failed=1
        fi
    done <<< "$uses_lines"
done
[[ $failed -eq 0 ]] && echo "  PASS: All 'uses:' references are SHA-pinned with version comments"
echo

# AC8.2: Check all actions/checkout uses have persist-credentials: false
echo "AC8.2: Checking all 'actions/checkout' uses have 'persist-credentials: false'..."
for file in "${workflow_files[@]}"; do
    # Find all actions/checkout uses lines
    checkout_lines=$(grep -n "actions/checkout" "$file" || true)
    if [[ -z "$checkout_lines" ]]; then
        continue
    fi

    while IFS= read -r line; do
        line_num=$(echo "$line" | cut -d: -f1)

        # Extract the with: block for this actions/checkout
        # Look from current line to next 'uses:' or next job definition
        tail_lines=$(($(wc -l < "$file") - line_num))
        if [[ $tail_lines -gt 0 ]]; then
            with_block=$(tail -n "$tail_lines" "$file" | head -n 30)

            # Check if persist-credentials: false exists in the with: block
            if ! echo "$with_block" | grep -q "persist-credentials:\s*false"; then
                uses_line=$(sed -n "${line_num}p" "$file")
                echo "  FAIL: $file:$line_num — actions/checkout missing 'persist-credentials: false'"
                echo "    Line: $uses_line"
                error_details="${error_details}AC8.2 FAIL ($file:$line_num): Missing 'persist-credentials: false'\n"
                failed=1
            fi
        fi
    done <<< "$checkout_lines"
done
[[ $failed -eq 0 ]] && echo "  PASS: All 'actions/checkout' uses have 'persist-credentials: false'"
echo

if [[ $failed -eq 1 ]]; then
    echo "========================================"
    echo "WORKFLOW SECURITY VERIFICATION FAILED"
    echo "========================================"
    echo -e "$error_details"
    exit 1
fi

echo "========================================"
echo "All workflow security checks passed!"
echo "  AC8.1: SHA-pinning with version comments ✓"
echo "  AC8.2: Checkout persist-credentials ✓"
echo "  AC8.3: Top-level permissions ✓"
echo "========================================"
exit 0
