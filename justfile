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

# Run all tests (parallel). Requires Docker or OrbStack — backend uses testcontainers for ephemeral PostgreSQL.
# macOS/OrbStack: export DOCKER_HOST=unix://$HOME/.orbstack/run/docker.sock
# Backend uses cargo-nextest (per backend/.config/nextest.toml) for cross-binary parallelism.
test:
	#!/usr/bin/env bash
	set -euo pipefail
	cd backend && cargo nextest run --workspace &
	pid1=$!
	(cd frontend && pnpm exec vitest run) &
	pid2=$!
	fail=0
	wait $pid1 || fail=1
	wait $pid2 || fail=1
	exit $fail

# Run Playwright E2E tests against the frontend dev server.
test-e2e:
	cd frontend && pnpm exec playwright test

# Tear down the persistent `atc-test-pg` container that the backend test
# suite leaves behind. Backend tests use testcontainers' reuse pattern
# (one container shared across all tests via per-test databases) which
# trades container-boot overhead for a long-lived container; over many
# `cargo test` invocations the container's stale `test_*` databases
# accumulate (~10 MB each). Run this recipe after wrapping up a heavy
# session, or anytime you want a clean slate. Safe to run if no
# container exists.
cleanup-test-pg:
	docker rm -f atc-test-pg 2>/dev/null || true

# Run performance verification: Tier 1 (vitest deterministic coalescing gate) +
# Tier 2 (Playwright frame-budget trace artifact). The Tier 1 test is also included
# in `just test`; this recipe runs both tiers together for local perf work.
test-perf:
	#!/usr/bin/env bash
	set -euo pipefail
	(cd frontend && pnpm exec vitest run src/lib/dispatcher.perf.browser.test.ts) &
	pid1=$!
	(cd frontend && pnpm exec playwright test e2e/frame-budget.test.ts) &
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

# Start the Grafana otel-lgtm all-in-one observability stack (collector, Tempo, Mimir, Loki, Grafana).
# Run alongside `just dev` after exporting OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
# in the dev shell. Grafana UI is at http://localhost:3000.
otel-dev-stack:
	docker compose -f compose.otel-dev.yaml up -d

# Stop and remove the otel-lgtm dev-stack container.
otel-dev-stack-stop:
	docker compose -f compose.otel-dev.yaml down

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
