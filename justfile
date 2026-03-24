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
	lefthook install

# Run all linters (stub — no code yet)
lint:
	@echo "lint: no code to lint yet"

# Format all code (stub — no code yet)
fmt:
	@echo "fmt: no code to format yet"

# Type-check / compile-check all code (stub — no code yet)
check:
	@echo "check: no code to check yet"

# Run all tests (stub — no code yet)
test:
	@echo "test: no tests to run yet"

# Start development servers (stub — no code yet)
dev:
	@echo "dev: no dev servers to start yet"

# Production build (stub — no code yet)
build:
	@echo "build: nothing to build yet"
