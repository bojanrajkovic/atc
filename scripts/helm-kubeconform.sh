#!/usr/bin/env bash
# Run helm template + kubeconform for every k8s × values combination and
# emit a Markdown summary to stdout and $GITHUB_STEP_SUMMARY.
#
# Loops over K8S_VERSIONS × deploy/helm/atc/tests/values-*.yaml, delegating
# each combination to scripts/helm-kubeconform-one.sh. Exits non-zero if any
# combination fails.
#
# Usage: scripts/helm-kubeconform.sh
set -euo pipefail

# K8s versions must match the kubeVersion range declared in Chart.yaml.
# Update here and in ci.yml when the chart's kubeVersion floor or
# latest-stable target changes.
K8S_VERSIONS=("1.32.0" "1.33.0")

chart_dir="deploy/helm/atc"
values_dir="${chart_dir}/tests"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Collect sorted values files into an indexed array (bash 3.2-compatible;
# mapfile/readarray require bash 4+).
values_files=()
while IFS= read -r f; do values_files+=("$f"); done \
  < <(find "${values_dir}" -name 'values-*.yaml' | sort)

# Parallel indexed arrays track results for each (k8s, values) pair.
# Associative arrays require bash 4+, so we use linear-search helpers below.
combo_keys=()
combo_results=()
combo_outputs=()
overall_rc=0

for k8s in "${K8S_VERSIONS[@]}"; do
  for values_file in "${values_files[@]}"; do
    values_name="$(basename "${values_file}" .yaml | sed 's/^values-//')"
    key="${k8s}/${values_name}"

    set +e
    output=$("${script_dir}/helm-kubeconform-one.sh" "${k8s}" "${values_file}" 2>&1)
    rc=$?
    set -e

    combo_keys+=("${key}")
    combo_outputs+=("${output}")
    if [[ $rc -eq 0 ]]; then
      combo_results+=("pass")
    else
      combo_results+=("fail")
      overall_rc=1
    fi
  done
done

pass_count=0
for r in "${combo_results[@]}"; do
  if [[ "$r" == "pass" ]]; then
    pass_count=$((pass_count + 1))
  fi
done
total=${#combo_results[@]}

_get_result() {
  local target="$1" i
  for i in "${!combo_keys[@]}"; do
    if [[ "${combo_keys[$i]}" == "$target" ]]; then
      echo "${combo_results[$i]}"
      return
    fi
  done
  echo "unknown"
}

_get_output() {
  local target="$1" i
  for i in "${!combo_keys[@]}"; do
    if [[ "${combo_keys[$i]}" == "$target" ]]; then
      printf '%s' "${combo_outputs[$i]}"
      return
    fi
  done
}

emit_summary() {
  local values_names=() values_file values_name k8s header separator row key has_failures r

  for values_file in "${values_files[@]}"; do
    values_names+=("$(basename "${values_file}" .yaml | sed 's/^values-//')")
  done

  echo "## Helm Validate"
  echo ""
  echo "**${pass_count} / ${total} combinations passed**"
  echo ""

  # 2D table: rows = values fixtures, columns = k8s versions.
  # Scanning across a row immediately reveals which version broke a fixture.
  header="| values |"
  separator="| --- |"
  for k8s in "${K8S_VERSIONS[@]}"; do
    header+=" \`${k8s}\` |"
    separator+=" --- |"
  done
  echo "${header}"
  echo "${separator}"

  for values_name in "${values_names[@]}"; do
    row="| \`${values_name}\` |"
    for k8s in "${K8S_VERSIONS[@]}"; do
      key="${k8s}/${values_name}"
      if [[ "$(_get_result "${key}")" == "pass" ]]; then
        row+=" ✅ |"
      else
        row+=" ❌ |"
      fi
    done
    echo "${row}"
  done

  has_failures=0
  for r in "${combo_results[@]}"; do
    if [[ "$r" == "fail" ]]; then
      has_failures=1
      break
    fi
  done

  if [[ $has_failures -eq 1 ]]; then
    echo ""
    echo "### Failures"
    for values_name in "${values_names[@]}"; do
      for k8s in "${K8S_VERSIONS[@]}"; do
        key="${k8s}/${values_name}"
        if [[ "$(_get_result "${key}")" == "fail" ]]; then
          echo ""
          echo "<details>"
          echo "<summary><code>${k8s} / ${values_name}</code></summary>"
          echo ""
          echo "<pre>"
          _get_output "${key}"
          echo "</pre>"
          echo "</details>"
        fi
      done
    done
  fi
}

if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  emit_summary | tee -a "${GITHUB_STEP_SUMMARY}"
else
  emit_summary
fi

exit $overall_rc
