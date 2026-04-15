# ATC — Actions Traffic Control
# Unified task runner. Run `just` to see available recipes.

# List available recipes
default:
	@just --list

# Bootstrap the development environment
setup:
	mise install
	pnpm install
	cd frontend && pnpm install
	lefthook install

# Run all linters (parallel)
lint:
	#!/usr/bin/env bash
	set -euo pipefail
	cd backend && cargo clippy --all-targets --all-features -- -D warnings &
	pid1=$!
	cd frontend && pnpm exec biome check . &
	pid2=$!
	cd frontend && pnpm exec eslint '**/*.svelte' &
	pid3=$!
	cd frontend && pnpm check &
	pid4=$!
	helm lint deploy/helm/atc &
	pid5=$!
	fail=0
	wait $pid1 || fail=1
	wait $pid2 || fail=1
	wait $pid3 || fail=1
	wait $pid4 || fail=1
	wait $pid5 || fail=1
	exit $fail

# Lint Helm chart
helm-lint:
	helm lint deploy/helm/atc

# Run helm-unittest suites
helm-unittest:
	helm unittest -f 'deploy/helm/atc/tests/unit/*.yaml' deploy/helm/atc

# Render Helm chart with test values (sanity check)
helm-template:
	#!/usr/bin/env bash
	set -euo pipefail
	for f in deploy/helm/atc/tests/values-*.yaml; do
		echo "==> helm template with $f"
		helm template atc deploy/helm/atc --values "$f" > /dev/null
	done

# Validate Helm chart with kubeconform across every tests/values-*.yaml fixture.
# Invoke with a positional Kubernetes version (just's standard convention):
#   just helm-check            # defaults to 1.29.0
#   just helm-check 1.32.0
helm-check kube_version="1.29.0":
	scripts/helm-kubeconform.sh {{kube_version}}

# Package Helm chart
helm-package:
	mkdir -p dist
	helm package deploy/helm/atc --destination ./dist

# Format all code (parallel)
fmt:
	#!/usr/bin/env bash
	set -euo pipefail
	cd backend && cargo fmt &
	pid1=$!
	cd frontend && pnpm exec biome format --write . &
	pid2=$!
	cd frontend && pnpm exec prettier --write '**/*.svelte' &
	pid3=$!
	fail=0
	wait $pid1 || fail=1
	wait $pid2 || fail=1
	wait $pid3 || fail=1
	exit $fail

# Type-check / compile-check all code (parallel)
check:
	#!/usr/bin/env bash
	set -euo pipefail
	cd backend && cargo check --workspace &
	pid1=$!
	cd frontend && pnpm build &
	pid2=$!
	cd frontend && pnpm check &
	pid3=$!
	just helm-check &
	pid4=$!
	just helm-unittest &
	pid5=$!
	fail=0
	wait $pid1 || fail=1
	wait $pid2 || fail=1
	wait $pid3 || fail=1
	wait $pid4 || fail=1
	wait $pid5 || fail=1
	exit $fail

# Run all tests (parallel)
test:
	#!/usr/bin/env bash
	set -euo pipefail
	cd backend && cargo test --workspace &
	pid1=$!
	(cd frontend && pnpm exec vitest run) &
	pid2=$!
	fail=0
	wait $pid1 || fail=1
	wait $pid2 || fail=1
	exit $fail

# Generate TypeScript types from Rust structs via ts-rs
types:
	#!/usr/bin/env bash
	set -euo pipefail
	mkdir -p frontend/src/lib/types/generated
	cd backend
	TS_RS_EXPORT_DIR="$(cd .. && pwd)/frontend/src/lib/types/generated" cargo test --workspace -- export_bindings
	echo "Types generated in frontend/src/lib/types/generated/"

# Start development servers (parallel — both run in foreground)
dev:
	#!/usr/bin/env bash
	cd frontend && pnpm dev &
	cd backend && cargo run -p atc-server &
	wait

# Audit dependencies for vulnerabilities and license compliance (parallel)
audit:
	#!/usr/bin/env bash
	set -euo pipefail
	cd backend && cargo deny check &
	pid1=$!
	cd backend && cargo audit &
	pid2=$!
	cd frontend && pnpm audit &
	pid3=$!
	fail=0
	wait $pid1 || fail=1
	wait $pid2 || fail=1
	wait $pid3 || fail=1
	exit $fail

# Production build (sequential — frontend must build before backend embeds it)
build:
	cd frontend && pnpm build
	cd backend && cargo build --release -p atc-server
