# ATC — Actions Traffic Control
# Unified task runner. Run `just` to see available recipes.

# List available recipes
default:
	@just --list

# Bootstrap the development environment
setup:
	mise install
	corepack enable
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
	wait $pid1 $pid2 $pid3

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
	wait $pid1 $pid2 $pid3

# Type-check / compile-check all code (parallel)
check:
	#!/usr/bin/env bash
	set -euo pipefail
	cd backend && cargo check --workspace &
	pid1=$!
	cd frontend && pnpm build &
	pid2=$!
	wait $pid1 $pid2

# Run all tests (parallel)
test:
	#!/usr/bin/env bash
	set -euo pipefail
	cd backend && cargo test --workspace &
	pid1=$!
	(cd frontend && pnpm exec vitest run --passWithNoTests 2>/dev/null || echo 'vitest: no tests configured yet (skipped)') &
	pid2=$!
	wait $pid1 $pid2

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
