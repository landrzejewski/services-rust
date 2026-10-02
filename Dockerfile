# syntax=docker/dockerfile:1
#
# Multi-stage build (step 024):
#   chef     – Rust toolchain + cargo-chef
#   planner  – computes a "recipe" (dependency list) from Cargo.toml/Cargo.lock
#   builder  – builds dependencies from the recipe (cached layer), then the application
#   runtime  – minimal image with only the binary and its config
#
# Build: docker build -t room-booking-service .
# Run:   docker compose --profile app up --build

ARG RUST_VERSION=1.99

FROM rust:${RUST_VERSION}-slim-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

# --- planner ---------------------------------------------------------------------------------
FROM chef AS planner
COPY . .
# recipe.json describes only the dependencies. It changes when Cargo.toml/Cargo.lock change –
# NOT when application code changes – so the expensive dependency layer below stays cached.
RUN cargo chef prepare --recipe-path recipe.json

# --- builder ---------------------------------------------------------------------------------
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build (and cache) all dependencies. Minutes on the first build, instant afterwards.
RUN cargo chef cook --release --recipe-path recipe.json

COPY . .
# sqlx macros check queries at compile time; there's no database during `docker build`,
# so they use the committed `.sqlx/` metadata (step 015).
ENV SQLX_OFFLINE=true
RUN cargo build --release --locked --bin rust-services

# --- runtime ---------------------------------------------------------------------------------
# distroless/cc: glibc, libgcc, CA certificates, tzdata – no shell, no package manager.
# Smaller attack surface; `:nonroot` runs as UID 65532 by default.
# Same Debian release as the builder (bookworm = debian12) -> compatible glibc.
FROM gcr.io/distroless/cc-debian12:nonroot AS runtime
WORKDIR /app

COPY --from=builder /app/target/release/rust-services /app/rust-services
# Configuration files; secrets are NOT baked into the image – they come from the environment.
COPY config /app/config

ENV APP_ENVIRONMENT=production
EXPOSE 3000
USER nonroot

# The binary checks itself (no curl in distroless) – see src/healthcheck.rs.
HEALTHCHECK --interval=10s --timeout=3s --start-period=10s --retries=3 \
    CMD ["/app/rust-services", "healthcheck"]

# Exec form: the binary is PID 1 and receives SIGTERM directly -> graceful shutdown (step 004).
ENTRYPOINT ["/app/rust-services"]
