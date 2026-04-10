#!/usr/bin/env bash
# Render a single helm values fixture and validate it with kubeconform.
#
# Usage: helm-kubeconform-one.sh <kube_version> <values_file>
#
# Example: scripts/helm-kubeconform-one.sh 1.29.0 deploy/helm/atc/tests/values-gateway.yaml
set -euo pipefail

kube_version="${1:?usage: helm-kubeconform-one.sh <kube_version> <values_file>}"
values_file="${2:?usage: helm-kubeconform-one.sh <kube_version> <values_file>}"
chart_dir="deploy/helm/atc"

# kubeconform pre-lowercases `.ResourceKind` (see kubeconform/pkg/registry/
# registry.go: `strings.ToLower(resourceKind)`), so this URL matches the
# lowercase filenames used by datreeio/CRDs-catalog (e.g. `httproute_v1.json`).
# Single-quoted so bash does not interpolate the Go-template braces.
schema_url='https://raw.githubusercontent.com/datreeio/CRDs-catalog/main/{{.Group}}/{{.ResourceKind}}_{{.ResourceAPIVersion}}.json'

echo "==> helm template + kubeconform (${values_file}, k8s ${kube_version})"
helm template atc "${chart_dir}" --values "${values_file}" \
  | kubeconform \
      -strict \
      -schema-location default \
      -schema-location "${schema_url}" \
      -kubernetes-version "${kube_version}" \
      -summary -
