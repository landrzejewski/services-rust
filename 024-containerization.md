# 024 – Deployment with containers

## Goal

Package the service as a small, secure container image and run the complete system (app,
PostgreSQL, Keycloak, Jaeger, Prometheus) with Docker Compose.

## Key concepts

### Multi-stage build

```
chef     rust:1.99-slim-bookworm + cargo-chef
planner  COPY . . → cargo chef prepare → recipe.json (dependencies only)
builder  cargo chef cook (deps, cached) → COPY . . → cargo build --release
runtime  distroless/cc-debian12:nonroot + binary + config            → ~46 MB
```

- The final image contains no compiler, sources, shell or package manager.
- **cargo-chef**: Docker caches layers by input. `recipe.json` changes only with
  `Cargo.toml`/`Cargo.lock`, so dependency compilation (the slow part) is reused when only
  application code changes (here: ~90 s cold build → ~40 s after a code change).
- `SQLX_OFFLINE=true` – query checking uses `.sqlx/` (no database at build time).
- `--locked` – exactly the versions from `Cargo.lock`.

### Runtime base images

| Base | Size | Contains | Notes |
|------|------|----------|-------|
| `debian:bookworm-slim` | ~75 MB | shell, apt | easy debugging; larger attack surface |
| **`gcr.io/distroless/cc-debian12`** | ~25 MB | glibc, libgcc, CA certs, tzdata | no shell; `:nonroot` user; matches a glibc build |
| `alpine` | ~8 MB | musl, shell | needs a musl build (`x86_64-unknown-linux-musl`); musl allocator slower → often `mimalloc` |
| `scratch` | 0 | nothing | fully static musl binary; add CA certs/tzdata yourself |

Builder and runtime must use compatible libc (same Debian release here).

### Image hardening

- run as non-root (`USER nonroot`, UID 65532),
- no secrets in the image (no `.env`, no keys) – `.dockerignore`, env/secret stores at runtime,
- minimal base, pinned tags (or digests), regular rebuilds for security patches,
- scan images: `trivy image room-booking-service`, `docker scout cves`,
- read-only root filesystem possible (`read_only: true`) – the service writes nothing to disk.

### Configuration and secrets

12-factor: one image for all environments, configuration via environment:

```yaml
environment:
  APP_ENVIRONMENT: production                 # config/production.toml: 0.0.0.0, JSON logs, 25 s grace
  DATABASE_URL: postgres://booking:booking@postgres:5432/booking
  APP_AUTH__JWT_SECRET: ${APP_AUTH__JWT_SECRET:-dev-only-...}
  APP_OIDC__DISCOVERY_URL: http://keycloak:8080/realms/room-booking/.well-known/openid-configuration
  APP_TELEMETRY__OTLP_ENDPOINT: http://jaeger:4318
```

Inside the compose network services are addressed by name. Keycloak keeps a fixed public issuer
(`KC_HOSTNAME=http://localhost:8180`) while the app fetches keys via `keycloak:8080`.
Production: Docker/Kubernetes secrets, Vault, cloud secret managers – not plain env files in git.

### Health checks without curl

```dockerfile
HEALTHCHECK --interval=10s --timeout=3s --start-period=10s --retries=3 \
    CMD ["/app/rust-services", "healthcheck"]
```

`rust-services healthcheck` opens a TCP connection to `127.0.0.1:$PORT`, sends
`GET /health/live` and exits 0/1 (`src/healthcheck.rs`). `docker compose ps` shows `(healthy)`;
`depends_on: condition: service_healthy` waits for PostgreSQL/Keycloak before starting the app.

Kubernetes uses probes instead of `HEALTHCHECK`:

```yaml
livenessProbe:  { httpGet: { path: /health/live,  port: 3000 } }
readinessProbe: { httpGet: { path: /health/ready, port: 3000 } }
```

### Graceful shutdown in containers

- exec-form `ENTRYPOINT ["/app/rust-services"]` → the binary is PID 1 and receives SIGTERM,
- the server stops accepting connections, finishes in-flight requests (step 004), closes the pool, flushes traces,
- `stop_grace_period` (compose) / `terminationGracePeriodSeconds` (k8s) must exceed
  `server.shutdown_grace_period_secs` (25 s in production) – otherwise SIGKILL cuts it short.

### Release profile

```toml
[profile.release]
lto = "thin"
codegen-units = 1
strip = "symbols"
# no panic = "abort": CatchPanicLayer needs unwinding
```

### Compose profiles

```bash
docker compose up -d                              # development: infrastructure only, app via cargo run
docker compose --profile app up -d --build        # full stack incl. the app container
docker compose --profile app logs -f app
docker compose --profile app down                 # stop all (add -v to drop data)
```

### Beyond compose (short)

- **CI**: `cargo fmt --check`, `clippy -D warnings`, `cargo test` (with a postgres service),
  `cargo sqlx prepare --check`, `cargo deny check`, build & push image, scan.
- **Kubernetes**: Deployment (replicas, resources, probes), Service, Ingress (TLS), ConfigMap/Secret,
  migrations as a Job / init container, HorizontalPodAutoscaler.
- **Multi-arch images**: `docker buildx build --platform linux/amd64,linux/arm64`.

## What changed in this branch

- `Dockerfile`, `.dockerignore` (new) – cargo-chef multi-stage build, distroless non-root runtime, HEALTHCHECK
- `src/healthcheck.rs` (new), `src/main.rs` – `healthcheck` subcommand, `ExitCode`
- `Cargo.toml` – `[profile.release]`
- `compose.yaml` – `app` service (profile `app`), env configuration, health dependencies, stop grace period
- `observability/prometheus.yml` – scrape target `app:3000`
- `README.md` – quick start

## Try it

```bash
docker build -t room-booking-service .
docker images room-booking-service
docker compose --profile app up -d --build
docker compose ps                                   # app (healthy)
curl localhost:3000/health/ready
curl -X POST localhost:3000/api/v1/auth/login -H 'content-type: application/json' \
  -d '{"email":"user@booking.local","password":"user-password-123"}'
docker compose logs app | tail                      # JSON logs
time docker compose stop app                        # graceful shutdown on SIGTERM
docker run --rm --entrypoint /app/rust-services room-booking-service healthcheck   # exit 1: nothing listening
```

## Exercises

1. Build a fully static musl binary (`--target x86_64-unknown-linux-musl`) and run it on `scratch` – what must be copied in additionally?
2. Run migrations as a separate one-shot compose service (`sqlx migrate run`) and set `APP_DATABASE__RUN_MIGRATIONS=false` for the app.
3. Use `docker buildx` with a cache mount (`--mount=type=cache,target=/usr/local/cargo/registry`) and compare build times.
