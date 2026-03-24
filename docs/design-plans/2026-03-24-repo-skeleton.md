# Repo Skeleton Design — Both Stacks Compile

## Summary

This phase bootstraps the "air traffic control" monorepo from a scaffold of stubs into a repository where both stacks genuinely compile, lint, and run. The backend is a Rust workspace with three crates: a placeholder for domain logic (`atc-core`), a placeholder for GitHub API integration (`atc-github`), and the only functional crate in this phase (`atc-server`), which runs an Axum HTTP server. The frontend is a standalone Svelte 5 + Vite single-page application with a complete OKLCH-based design system offering four themes. Neither stack contains application features yet — the goal is a solid foundation that future phases can build on without revisiting tooling.

The approach is deliberately layered: backend compiles first (Phase 1), then the frontend builds independently (Phase 2), then the root justfile and Lefthook hooks are wired to real commands (Phase 3), and finally CLAUDE.md and architecture documentation are updated to reflect the new layout (Phase 4). The two stacks are kept genuinely independent — they share no build system, lockfile, or lint configuration. The only coupling is the asset-serving contract: `atc-server` embeds `frontend/dist/` at compile time in release mode, and proxies to the Vite dev server at `:5173` in dev mode.

## Definition of Done

Phase 2 is done when:

1. **Backend compiles:** Rust workspace at `backend/` with 3 crates (atc-core, atc-github, atc-server). atc-server runs an Axum server on :8080 with a health endpoint and embeds the frontend build via rust-embed. Other two crates are placeholder modules.

2. **Frontend compiles:** Svelte 5 + Vite + Tailwind v4 app at `frontend/` with OKLCH design tokens from .impeccable.md. Hello-world App.svelte renders. Biome for TS/JS, eslint-plugin-svelte + prettier-plugin-svelte for .svelte files.

3. **Tooling works:** `just lint`, `just fmt`, `just check`, `just test`, `just dev`, `just build` all run real commands (parallel where applicable). Lefthook pre-commit/pre-push hooks run against actual code. No more stub recipes or conditional skips.

4. **Documentation follows conventions:** CLAUDE.md updated with new project structure. Doc-mapping.sh populated with backend/frontend paths. Five-layer model respected — architecture docs created as appropriate.

## Acceptance Criteria

### repo-skeleton.AC1: Backend Rust workspace compiles
- **repo-skeleton.AC1.1 Success:** `cargo check --workspace` passes from `backend/`
- **repo-skeleton.AC1.2 Success:** `cargo test --workspace` passes from `backend/`
- **repo-skeleton.AC1.3 Success:** `cargo run -p atc-server` starts and binds to `:8080`
- **repo-skeleton.AC1.4 Success:** `GET /health` returns HTTP 200 with JSON body
- **repo-skeleton.AC1.5 Success:** In release mode, embedded frontend files are served at `/`
- **repo-skeleton.AC1.6 Success:** SPA fallback: non-file paths (e.g., `/dashboard`) return `index.html`
- **repo-skeleton.AC1.7 Success:** In dev mode, non-API requests proxy to Vite at `localhost:5173`
- **repo-skeleton.AC1.8 Edge:** `backend/Cargo.lock` is tracked in git (not ignored)

### repo-skeleton.AC2: Frontend builds and renders
- **repo-skeleton.AC2.1 Success:** `pnpm dev` starts Vite dev server at `:5173` from `frontend/`
- **repo-skeleton.AC2.2 Success:** `pnpm build` produces `frontend/dist/` with index.html and assets
- **repo-skeleton.AC2.3 Success:** App.svelte renders with OKLCH-themed styling (Tailwind tokens work)
- **repo-skeleton.AC2.4 Success:** All 4 themes (Warm, Radar, Violet, Pink) switch via `data-theme` attribute
- **repo-skeleton.AC2.5 Success:** Dark and light mode tokens apply correctly
- **repo-skeleton.AC2.6 Success:** `pnpm exec biome check .` passes from `frontend/`
- **repo-skeleton.AC2.7 Success:** `pnpm exec eslint '**/*.svelte'` passes from `frontend/`
- **repo-skeleton.AC2.8 Success:** `pnpm exec prettier --check '**/*.svelte'` passes from `frontend/`

### repo-skeleton.AC3: Task runner recipes work
- **repo-skeleton.AC3.1 Success:** `just setup` completes from a clean clone (installs all deps + hooks)
- **repo-skeleton.AC3.2 Success:** `just lint` runs clippy + biome + eslint in parallel, all pass
- **repo-skeleton.AC3.3 Success:** `just fmt` runs cargo fmt + biome format + prettier in parallel, all pass
- **repo-skeleton.AC3.4 Success:** `just check` runs cargo check + pnpm build in parallel, all pass
- **repo-skeleton.AC3.5 Success:** `just test` runs cargo test (+ vitest if configured) in parallel
- **repo-skeleton.AC3.6 Success:** `just dev` starts both Vite (:5173) and Axum (:8080) in parallel
- **repo-skeleton.AC3.7 Success:** `just build` builds frontend first, then backend release with embedded assets
- **repo-skeleton.AC3.8 Failure:** `just build` with empty `frontend/dist/` produces a binary that serves no frontend content (not a silent success)

### repo-skeleton.AC4: Git hooks run against real code
- **repo-skeleton.AC4.1 Success:** Committing a `.rs` file triggers clippy and rustfmt (not skipped)
- **repo-skeleton.AC4.2 Success:** Committing a `.ts` file triggers biome check (not skipped)
- **repo-skeleton.AC4.3 Success:** Committing a `.svelte` file triggers eslint-svelte (not skipped)
- **repo-skeleton.AC4.4 Success:** Pre-push runs `cargo test --workspace` from `backend/`
- **repo-skeleton.AC4.5 Success:** Pre-push doc-staleness gate still works

### repo-skeleton.AC5: Documentation follows conventions
- **repo-skeleton.AC5.1 Success:** CLAUDE.md says "Svelte 5 + Vite" (not SvelteKit), status updated
- **repo-skeleton.AC5.2 Success:** `docs/architecture/backend-server.md` exists with all 4 required anchors + timestamp
- **repo-skeleton.AC5.3 Success:** `docs/architecture/frontend-app.md` exists with all 4 required anchors + timestamp
- **repo-skeleton.AC5.4 Success:** `scripts/doc-mapping.sh` maps `backend/crates/atc-server/src/*` and `frontend/src/*` to their architecture docs
- **repo-skeleton.AC5.5 Success:** Modifying a mapped source file without updating its architecture doc is flagged by pre-push hook

## Glossary

- **Axum**: A Rust web framework built on top of Tokio and Tower, used here to implement the HTTP server with routing and middleware support.
- **Biome**: A fast Rust-based formatter and linter for JavaScript and TypeScript, used here for `.ts`/`.js` files in the frontend.
- **Cargo workspace**: A Cargo feature that groups multiple Rust crates under a single `Cargo.toml`, allowing shared dependency version declarations and unified build/test commands.
- **`cfg!(debug_assertions)`**: A Rust compile-time flag that is true in debug builds and false in release builds; used here to switch between dev-proxy mode and embedded-asset mode.
- **clippy**: The official Rust linter; produces warnings and errors about common mistakes and non-idiomatic code.
- **crate**: A compilation unit in Rust — either a library (`lib.rs`) or a binary (`main.rs`). The three crates in this phase are `atc-core`, `atc-github`, and `atc-server`.
- **eslint-plugin-svelte**: An ESLint plugin that understands `.svelte` file syntax and enforces Svelte-specific linting rules. Used alongside Biome because Biome does not fully handle `.svelte` files.
- **five-layer model**: A documentation architecture convention used in this project that separates docs into distinct layers (architecture docs, contributing guide, AI agent index, directive extracts, ideation docs) with a non-duplication rule.
- **HMR (Hot Module Replacement)**: A Vite feature that pushes code changes to the browser without a full page reload during development.
- **justfile / just**: A command runner (similar to Make) that defines named recipes. Used here as the root-level task runner to orchestrate lint, build, test, and dev commands across both stacks.
- **Lefthook**: A Git hook manager that runs shell commands on pre-commit and pre-push events. Configured via `lefthook.yml`; the `root:` directive sets the working directory per command.
- **`[workspace.dependencies]`**: A Cargo workspace feature that centralizes dependency version declarations so individual crates can reference them without repeating version numbers.
- **mise**: A polyglot runtime version manager used in `just setup` to install the correct versions of Rust, Node, and other tools.
- **OKLCH**: A perceptual color model (Lightness, Chroma, Hue) used in CSS. More perceptually uniform than HSL; used here to define the design system's theme tokens via a single `--hue` variable that drives all four themes.
- **`@tailwindcss/vite`**: The Vite plugin for Tailwind v4, which replaces the PostCSS-based integration from earlier Tailwind versions.
- **`@theme` block**: A Tailwind v4 CSS directive that declares design tokens (custom properties) as part of the Tailwind configuration, written directly in CSS rather than a `tailwind.config.js` file.
- **pnpm catalog**: A `pnpm-workspace.yaml` feature that centralizes dependency version pins so all packages in a pnpm workspace reference versions from one place.
- **prettier-plugin-svelte**: A Prettier plugin for formatting `.svelte` files. Used alongside Biome because Biome does not format `.svelte` files.
- **reqwest**: A Rust HTTP client library; used here in dev mode to proxy non-API requests from Axum to the Vite dev server.
- **rust-embed**: A Rust crate that embeds files from a directory into the compiled binary at build time using a derive macro (`RustEmbed`). Used to bundle `frontend/dist/` into the release binary.
- **SPA fallback**: A server behavior where any request path that does not match a static file is served `index.html`, allowing client-side routing to handle the path.
- **Svelte 5**: The fifth major version of the Svelte component framework. Used here as a standalone app (not SvelteKit), meaning it produces a static bundle without server-side rendering or file-based routing.
- **Vite**: A frontend build tool and dev server used to bundle and serve the Svelte app.
- **Vitest**: A Vite-native unit test runner for JavaScript/TypeScript. Installed in the skeleton but not actively used until future phases have testable logic.

## Architecture

Monorepo with two self-contained stacks: `backend/` (Rust workspace) and `frontend/` (pnpm workspace). Root owns only orchestration — justfile, lefthook, commitlint. Each stack manages its own dependencies, lockfiles, and lint configs.

**Backend:** Cargo workspace at `backend/Cargo.toml` with three crates under `backend/crates/`. Shared dependency versions via `[workspace.dependencies]`. `atc-server` is the only functional crate in this phase — it runs an Axum HTTP server on `:8080` with a `/health` endpoint and serves the frontend via rust-embed (release) or proxies to Vite (dev).

**Frontend:** pnpm workspace at `frontend/pnpm-workspace.yaml` with a `catalog:` section for centralized version pins. Single Svelte 5 + Vite app producing a static build in `frontend/dist/`. Tailwind v4 via `@tailwindcss/vite` plugin (no PostCSS). Complete OKLCH design system with 4 themes (Warm, Radar, Violet, Pink) driven by a single `--hue` CSS variable.

**Asset serving strategy:**
- **Release mode** (`cfg!(not(debug_assertions))`): rust-embed derives `RustEmbed` on `frontend/dist/`, serves embedded files with SPA fallback (non-file paths return `index.html`)
- **Dev mode** (`cfg!(debug_assertions)`): Axum proxies non-API requests to Vite dev server at `localhost:5173` via reqwest. Vite handles HMR.

**Tooling orchestration:** justfile at root runs backend and frontend commands in parallel (except `just build` which is sequential — frontend must build before rust-embed can embed). Lefthook uses `root:` per-command to run linters in the correct subdirectory.

## Existing Patterns

Investigation of the Phase 1 codebase found:

- **justfile** with 8 recipes (all stubs except `setup`). Phase 2 replaces stubs with real parallel commands.
- **lefthook.yml** with three-tier hooks already configured. Pre-commit commands already specify correct glob filters (`*.rs`, `*.{ts,js}`, `*.svelte`). Phase 2 adds `root:` directives and removes conditional skips from pre-push.
- **package.json** at root with commitlint devDeps and `packageManager: "pnpm@10.32.1"`. Phase 2 does NOT move these — root keeps commitlint, frontend gets its own package.json.
- **.gitignore** already covers `target/`, `node_modules/`, `dist/`. Phase 2 modifies `Cargo.lock` handling: un-ignore for `backend/Cargo.lock` (binary project should track lockfile).
- **scripts/doc-mapping.sh** scaffolded with empty mappings and example patterns showing `backend/src/*` and `frontend/src/*` — Phase 2 populates these.

No divergence from existing patterns. Phase 2 builds on Phase 1's scaffolding without restructuring.

## Implementation Phases

<!-- START_PHASE_1 -->
### Phase 1: Backend Rust Workspace

**Goal:** Cargo workspace compiles with three crates. `cargo check` and `cargo test` pass (no-op tests).

**Components:**
- `backend/Cargo.toml` — workspace definition with `members = ["crates/*"]` and `[workspace.dependencies]` for shared versions (axum, tokio, serde, rust-embed, reqwest, tower-http)
- `backend/Cargo.lock` — tracked in git (binary project)
- `backend/rustfmt.toml` — Rust formatting config
- `backend/clippy.toml` — Clippy config (defaults or project preferences)
- `backend/crates/atc-core/` — `Cargo.toml` + `src/lib.rs` placeholder (domain logic, no deps)
- `backend/crates/atc-github/` — `Cargo.toml` + `src/lib.rs` placeholder (GitHub API, no deps)
- `backend/crates/atc-server/` — `Cargo.toml` with workspace deps + `src/main.rs` entry point, `src/routes.rs` health endpoint, `src/assets.rs` rust-embed + dev proxy

**Dependencies:** Phase 1 bootstrap (existing repo)

**.gitignore update:** Change `Cargo.lock` handling to `Cargo.lock` (root ignored) + `!backend/Cargo.lock` (tracked)

**Done when:** `cargo check --workspace` passes from `backend/`. `cargo test --workspace` passes. `cargo run -p atc-server` starts Axum on `:8080` and `GET /health` returns 200. In release mode, serves embedded frontend files with SPA fallback. In dev mode, proxies to `localhost:5173`.
<!-- END_PHASE_1 -->

<!-- START_PHASE_2 -->
### Phase 2: Frontend Svelte + Vite App

**Goal:** Frontend builds, dev server starts, Tailwind + OKLCH tokens render correctly.

**Components:**
- `frontend/pnpm-workspace.yaml` — workspace with `catalog:` version pins for svelte, vite, tailwindcss, biome, eslint-plugin-svelte, prettier, prettier-plugin-svelte
- `frontend/package.json` — deps referencing `catalog:` protocol, scripts for dev/build/preview
- `frontend/vite.config.ts` — `@tailwindcss/vite` plugin (before `@sveltejs/vite-plugin-svelte`)
- `frontend/svelte.config.js` — minimal config (no SvelteKit)
- `frontend/tsconfig.json` — TypeScript config for Svelte
- `frontend/biome.json` — lint + format for `.ts`/`.js`, prettier compat mode
- `frontend/.eslintrc.cjs` — eslint-plugin-svelte for `.svelte` linting
- `frontend/.prettierrc` — prettier-plugin-svelte for `.svelte` formatting
- `frontend/src/app.css` — `@import "tailwindcss"` + `@theme` block with full OKLCH system: 4 themes (Warm hue 70, Radar hue 155, Violet hue 280, Pink hue 310), dark/light mode semantic tokens, type scale, status colors
- `frontend/src/main.ts` — Svelte mount point
- `frontend/src/App.svelte` — hello-world component demonstrating Tailwind + OKLCH tokens work (styled heading with theme colors)

**Dependencies:** None (frontend is independent of backend for build)

**Done when:** `pnpm dev` starts Vite at `:5173`. `pnpm build` produces `frontend/dist/`. App.svelte renders with OKLCH-themed styling. `pnpm exec biome check .` passes. `pnpm exec eslint '**/*.svelte'` passes. `pnpm exec prettier --check '**/*.svelte'` passes.
<!-- END_PHASE_2 -->

<!-- START_PHASE_3 -->
### Phase 3: Tooling Integration

**Goal:** Justfile recipes are real, lefthook hooks work against actual code, `just setup` bootstraps everything.

**Components:**
- `justfile` — replace all stubs with real commands:
  - `setup`: mise install + corepack enable + pnpm install (root) + cd frontend && pnpm install + lefthook install
  - `lint`: parallel cargo clippy (backend/) + biome check (frontend/) + eslint .svelte (frontend/)
  - `fmt`: parallel cargo fmt (backend/) + biome format (frontend/) + prettier .svelte (frontend/)
  - `check`: parallel cargo check (backend/) + pnpm build (frontend/)
  - `test`: parallel cargo test (backend/) + vitest run (frontend/) — vitest may be a stub if not configured
  - `dev`: parallel pnpm dev (frontend/) + cargo run (backend/)
  - `build`: sequential pnpm build (frontend/) THEN cargo build --release (backend/)
- `lefthook.yml` — add `root: backend/` to clippy/rustfmt commands, `root: frontend/` to biome/eslint commands. Remove conditional `if [ -f Cargo.toml ]` from pre-push cargo-test (replace with `root: backend/`). Remove vitest conditional (replace with `root: frontend/`). Playwright conditional remains (not configured).

**Dependencies:** Phase 1 (backend compiles), Phase 2 (frontend builds)

**Done when:** `just setup` from a clean clone completes. `just lint`, `just fmt`, `just check` all pass. `just dev` starts both servers. `just build` produces release binary with embedded frontend. Lefthook pre-commit runs clippy + biome + eslint against actual code. Pre-push runs cargo test + vitest (or skips vitest gracefully if not configured).
<!-- END_PHASE_3 -->

<!-- START_PHASE_4 -->
### Phase 4: Documentation

**Goal:** CLAUDE.md, architecture docs, and doc-mapping reflect the new project structure. Five-layer model followed.

**Components:**
- `CLAUDE.md` — update: fix "SvelteKit" → "Svelte 5 + Vite", update status, add backend/frontend to Project Structure, remove "stubs" note from Commands
- `docs/architecture/backend-server.md` — new architecture doc: Purpose (Axum HTTP + asset serving), Key Decisions (rust-embed, cfg debug_assertions, reqwest proxy), Boundaries (HTTP routing, NOT domain logic or GitHub API), Files (backend/crates/atc-server/src/*)
- `docs/architecture/frontend-app.md` — new architecture doc: Purpose (Svelte 5 SPA, OKLCH design system), Key Decisions (standalone Svelte not SvelteKit, @tailwindcss/vite, Biome + eslint split, 4-theme OKLCH), Boundaries (UI rendering + tokens, NOT API or state management), Files (frontend/src/*, frontend/vite.config.ts, frontend/biome.json)
- `scripts/doc-mapping.sh` — populate mappings: `backend/crates/atc-server/src/*` → `docs/architecture/backend-server.md`, `frontend/src/*` → `docs/architecture/frontend-app.md`

**Dependencies:** Phase 1 (backend paths exist), Phase 2 (frontend paths exist)

**Done when:** All three docs exist with required anchor sections (Purpose, Key Decisions, Boundaries, Files, Last verified timestamp). Doc-mapping.sh returns correct paths. `scripts/check-docs-lefthook.sh` would flag missing doc updates on relevant source changes.
<!-- END_PHASE_4 -->

## Additional Considerations

**Vitest in skeleton phase:** The frontend has no application logic to test yet. `just test` on the frontend side may be a no-op or run against a trivial smoke test. The vitest devDep should be installed so the tooling pipeline is complete, but zero tests is acceptable for a skeleton. The lefthook pre-push vitest command should handle this gracefully (exit 0 when no tests exist).

**`just build` ordering is critical:** Frontend must build before backend release build. rust-embed reads `frontend/dist/` at compile time — if the directory is empty or stale, the binary will serve nothing. The justfile `build` recipe enforces this by running sequentially.

**Dev proxy scope:** The reqwest proxy in dev mode forwards all non-API requests to Vite. "API requests" are defined as paths starting with `/api/` or `/health`. Everything else goes to Vite. This convention is established in the skeleton and future phases follow it.
