# 003 – Development environment

## Goal

Set up a repeatable development environment: toolchain, IDE, formatting, linting,
fast feedback loop, environment variables and local infrastructure.

## Key concepts

### Toolchain – rustup

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup update
rustup show            # active toolchain (honours rust-toolchain.toml)
```

`rust-toolchain.toml` in the project root makes every developer and CI use the same channel and
components – rustup switches/installs automatically. Pin an exact version (`"1.98.1"`) for
fully reproducible builds.

### IDE

| IDE | Setup |
|-----|-------|
| RustRover (JetBrains) | built-in Rust support, debugger, HTTP client for `requests/*.http` |
| VS Code | extensions: *rust-analyzer*, *CodeLLDB* (debugging), *Even Better TOML*, *REST Client* |
| Others (Zed, Neovim, Helix) | rust-analyzer via LSP |

Recommended rust-analyzer setting – run clippy instead of `cargo check` on save:
`"rust-analyzer.check.command": "clippy"`.

### Formatting – rustfmt

```bash
cargo fmt            # format
cargo fmt --check    # verify (CI)
```

Configured in `rustfmt.toml`. Use only stable options so formatting does not depend on nightly.

### Linting – clippy

```bash
cargo clippy --all-targets                 # lib, bin, tests, examples
cargo clippy --all-targets -- -D warnings  # CI: warnings become errors
```

Lint levels live in `Cargo.toml`:

```toml
[lints.rust]
unsafe_code = "forbid"

[lints.clippy]
all = { level = "warn", priority = -1 }   # group – lower priority so single lints can override it
unwrap_used = "warn"
```

Lint *options* (thresholds, exceptions) go to `clippy.toml`, e.g. `allow-unwrap-in-tests = true`.
Useful groups to consider: `pedantic` (strict, opinionated), `nursery` (experimental),
`restriction` (pick individual lints only).

### Fast feedback loop

| Tool | Command | Purpose |
|------|---------|---------|
| bacon | `bacon`, `bacon test`, `bacon run` | re-run check/clippy/tests/server on change (config: `bacon.toml`) |
| watchexec | `watchexec -r -e rs,toml cargo run` | generic file watcher, `-r` restarts the process |
| cargo-watch | `cargo watch -x run` | older alternative, no longer actively developed |

Install: `cargo install --locked bacon`.

### Useful cargo extensions

| Tool | Purpose |
|------|---------|
| `cargo expand` | show code after macro expansion (`cargo expand main` – see what `#[tokio::main]` generates) |
| `cargo add` / `cargo remove` | manage dependencies (built into cargo) |
| `cargo tree` | dependency tree, `cargo tree -d` – duplicated crates |
| `cargo outdated` / `cargo upgrade` | find/upgrade outdated dependencies |
| `cargo audit` | check `Cargo.lock` against the RustSec vulnerability database |
| `cargo deny` | licenses, banned crates, advisories, duplicate versions – policy checks for CI |
| `cargo nextest` | faster test runner with better output |
| `sqlx-cli` | database migrations, offline query metadata (step 014+) |

### Environment variables and `.env`

Twelve-factor rule: configuration lives in the environment, not in code.

```rust
dotenvy::dotenv().ok();                         // load .env if present, never overrides real env
let addr = std::env::var("APP_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".into());
```

- `.env` – local values, **git-ignored** (may contain secrets),
- `.env.example` – committed template documenting all variables.

`dotenvy` is the maintained fork of the original `dotenv` crate.

### Local infrastructure – Docker Compose

`compose.yaml` describes services needed during development (PostgreSQL now, Keycloak and
observability tools later). The application runs on the host for fast iteration.

```bash
docker compose up -d        # start in background
docker compose ps           # status (incl. healthchecks)
docker compose logs -f postgres
docker compose down         # stop, keep data;  down -v  – also delete volumes
```

### Build profiles (for reference)

`cargo build` uses the `dev` profile (fast compile, debug info), `cargo build --release` the
`release` profile (optimized). Profiles can be tuned in `Cargo.toml` (`[profile.dev]`,
`[profile.release]`) – used in step 024 for the production image.

## What changed in this branch

- `rust-toolchain.toml` – toolchain channel and components
- `rustfmt.toml`, `.editorconfig` – formatting
- `Cargo.toml` – `[lints]` section, dependency `dotenvy`; `clippy.toml` – lint options
- `bacon.toml` – watch jobs
- `.env.example` (+ local `.env`, git-ignored) – environment variables
- `compose.yaml` – PostgreSQL for later steps
- `src/main.rs` – loads `.env`, bind address from `APP_ADDR`, `unwrap` replaced with `expect`

## Try it

```bash
cp .env.example .env
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
APP_ADDR=127.0.0.1:3100 cargo run          # real env wins over .env
curl localhost:3100/health

cargo expand main | head -40               # requires: cargo install cargo-expand
docker compose up -d && docker compose ps
```

## Exercises

1. Add `clippy::pedantic` as `warn` and review what it reports; decide which lints to allow.
2. Configure your IDE to format on save and run clippy as the checker.
3. Add a `bacon` job running `cargo doc --no-deps` and bind it to the `d` key.
