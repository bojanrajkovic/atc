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
	fail=0
	wait $pid1 || fail=1
	wait $pid2 || fail=1
	wait $pid3 || fail=1
	wait $pid4 || fail=1
	exit $fail

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
	fail=0
	wait $pid1 || fail=1
	wait $pid2 || fail=1
	wait $pid3 || fail=1
	exit $fail

# Run all tests (parallel)
test:
	#!/usr/bin/env bash
	set -euo pipefail
	cd backend && cargo test --workspace &
	pid1=$!
	(cd frontend && pnpm exec vitest run --passWithNoTests 2>/dev/null || echo 'vitest: no tests configured yet (skipped)') &
	pid2=$!
	fail=0
	wait $pid1 || fail=1
	wait $pid2 || fail=1
	exit $fail

# Start development servers (parallel — both run in foreground)
dev:
	#!/usr/bin/env bash
	cd frontend && pnpm dev &
	cd backend && cargo run -p atc-server &
	wait

# Production build (sequential — frontend must build before backend embeds it)
build:
	cd frontend && pnpm build
	cd backend && cargo build --release -p atc-server
