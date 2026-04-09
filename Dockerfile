# Build args — .mise.toml is the version authority. CI passes these from mise;
# defaults here allow standalone `docker build` without extra flags.
# IMPORTANT: Keep defaults in sync with .mise.toml when tool versions change.
ARG RUST_VERSION=1.94
ARG NODE_VERSION=25

# ---- Stage 1: Frontend ----
# Build Svelte SPA with Vite. Output: /app/frontend/dist/
#
# pnpm version is pinned via the "packageManager" field in frontend/package.json.
# Corepack auto-downloads and activates that exact version on first pnpm
# invocation, so the container build uses the same pnpm as local dev and CI.
# COREPACK_ENABLE_DOWNLOAD_PROMPT=0 suppresses the interactive consent prompt.
FROM node:${NODE_VERSION}-slim AS frontend
ENV COREPACK_ENABLE_DOWNLOAD_PROMPT=0
RUN npm install -g corepack --force && corepack enable
WORKDIR /app/frontend
COPY frontend/pnpm-lock.yaml frontend/package.json frontend/pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY frontend/ .
RUN pnpm build

# ---- Stage 2: Planner ----
# Generate cargo-chef recipe from workspace manifests.
ARG RUST_VERSION
FROM rust:${RUST_VERSION}-slim AS planner
RUN cargo install cargo-chef --locked
WORKDIR /app/backend
COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/crates/atc-core/Cargo.toml crates/atc-core/Cargo.toml
COPY backend/crates/atc-github/Cargo.toml crates/atc-github/Cargo.toml
COPY backend/crates/atc-server/Cargo.toml crates/atc-server/Cargo.toml
RUN mkdir -p crates/atc-core/src crates/atc-github/src crates/atc-server/src \
    && touch crates/atc-core/src/lib.rs crates/atc-github/src/lib.rs crates/atc-server/src/main.rs
RUN cargo chef prepare --recipe-path /app/recipe.json

# ---- Stage 3: Dependencies ----
# Compile dependencies from recipe. This layer caches when Cargo.toml/Cargo.lock unchanged.
ARG RUST_VERSION
FROM rust:${RUST_VERSION}-slim AS deps
COPY --from=planner /usr/local/cargo/bin/cargo-chef /usr/local/cargo/bin/cargo-chef
WORKDIR /app/backend
COPY --from=planner /app/recipe.json /app/recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git/db \
    cargo chef cook --release --locked --recipe-path /app/recipe.json

# ---- Stage 4: Builder ----
# Copy real source + frontend assets, compile the server binary.
ARG RUST_VERSION
FROM rust:${RUST_VERSION}-slim AS builder
WORKDIR /app
COPY --from=deps /app/backend/target backend/target
COPY backend/ backend/
COPY --from=frontend /app/frontend/dist frontend/dist/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git/db \
    cd backend && cargo build --release --locked --bin atc-server

# ---- Stage 5: Runtime ----
# Distroless minimal image (~32 MB). No shell, no package manager.
FROM gcr.io/distroless/cc-debian13:nonroot
COPY --from=builder /app/backend/target/release/atc-server /atc-server
USER 65532:65532
EXPOSE 8080
EXPOSE 9090
ENTRYPOINT ["/atc-server"]
