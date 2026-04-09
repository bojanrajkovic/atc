#!/usr/bin/env bash
# Run `helm template` for each values fixture and pipe the rendered manifest
# through kubeconform with the datreeio/CRDs-catalog schema overlay.
#
# Usage: helm-kubeconform.sh <kube_version>
#
# Example: scripts/helm-kubeconform.sh 1.29.0
set -euo pipefail

kube_version="${1:-1.29.0}"
chart_dir="deploy/helm/atc"
values_dir="${chart_dir}/tests"

# kubeconform uses Go text/template syntax to interpolate resource metadata
# into the schema URL. Single-quoted so bash does not touch the braces.
# `ResourceKind` is pre-lowercased by kubeconform itself (see
# kubeconform/pkg/registry/registry.go: `strings.ToLower(resourceKind)`), so
# no filter is needed here and the URL matches the lowercase filenames used
# by the datreeio/CRDs-catalog repository (e.g. `httproute_v1.json`).
schema_url='https://raw.githubusercontent.com/datreeio/CRDs-catalog/main/{{.Group}}/{{.ResourceKind}}_{{.ResourceAPIVersion}}.json'

for values_file in "${values_dir}"/values-*.yaml; do
  echo "==> helm template + kubeconform (${values_file}, k8s ${kube_version})"
  helm template atc "${chart_dir}" --values "${values_file}" \
    | kubeconform \
        -strict \
        -schema-location default \
        -schema-location "${schema_url}" \
        -kubernetes-version "${kube_version}" \
        -summary -
done
