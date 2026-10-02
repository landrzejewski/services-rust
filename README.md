# Room Booking Service – Rust & Axum training

A single application developed step by step during the course
*"Rust – building services and business applications"*.
Every step lives on its own branch; each branch builds on the previous one.

## Domain

A REST service for booking rooms (meeting rooms, conference halls, etc.).

| Entity    | Description                                           |
|-----------|-------------------------------------------------------|
| `User`    | Person using the system, role `USER` or `ADMIN`       |
| `Room`    | Bookable resource: name, capacity, opening hours      |
| `Booking` | Reservation of a room by a user for a time range      |

Business rules (implemented gradually):

- bookings of the same room must not overlap,
- a booking must fit within the room's opening hours,
- a user may hold at most N active bookings,
- a user may cancel only their own bookings, an admin may cancel any.

## How to use this repository

```bash
git branch -a                 # list all steps
git switch 001-axum-overview  # go to a given step
git diff 001-axum-overview 002-tokio-runtime  # see what a step added
```

Each branch contains:

- `NNN-topic.md` – theory for the step (files accumulate, older steps stay available),
- comments in the code explaining every newly introduced element,
- `requests/NNN-topic.http` – sample requests (RustRover/IntelliJ HTTP client, VS Code REST Client).

## Requirements

- Rust (stable, edition 2024) – `rustup` recommended
- Docker + Docker Compose (from step 014)
- `curl` or an HTTP client (`jq` helps)

## Quick start (final state)

```bash
cp .env.example .env
docker compose up -d                          # PostgreSQL, Keycloak, Jaeger, Prometheus
cargo run                                     # the service on http://localhost:3000
cargo test                                    # unit, API and database tests

docker compose --profile app up -d --build    # or: everything in containers
```

| Service | URL | Credentials |
|---------|-----|-------------|
| API | http://localhost:3000/api/v1 | `user@booking.local` / `user-password-123`, `admin@booking.local` / `admin-password-123` |
| Keycloak | http://localhost:8180 | console `admin` / `admin`; realm users `alice` / `alice-password`, `bob` / `bob-password` |
| Jaeger | http://localhost:16686 | – |
| Prometheus | http://localhost:9090 | – |

## Branch index

| Branch | Topic |
|--------|-------|
| **Module 1 – Introduction to Axum** | |
| `001-axum-overview` | Axum characteristics and key building blocks |
| `002-tokio-runtime` | Tokio as the async runtime for Axum |
| `003-dev-environment` | Development environment setup |
| `004-http-server` | Creating, configuring and running the HTTP server |
| `005-layered-architecture` | Recommended architecture – layers of responsibility |
| **Module 2 – Building REST services** | |
| `006-routing` | Routing and request handling |
| `007-serialization` | Serialization and deserialization |
| `008-dto-mapping` | DTOs and mapping between layers |
| `009-validation` | Input validation |
| `010-error-handling` | Error handling |
| `011-middleware` | Middleware – enriching requests and responses |
| **Module 3 – Business logic and persistence** | |
| `012-dependency-injection` | Dependency injection |
| `013-business-logic` | Business logic |
| `014-database-configuration` | Database connection configuration |
| `015-data-access-sqlx` | Persistence with sqlx |
| `016-transactions` | Transactions |
| `017-sea-orm-diesel-async` | sea-orm and diesel-async – selection criteria |
| **Module 4 – Security** | |
| `018-security-fundamentals` | Authentication, authorization, integrity, confidentiality |
| `019-jwt` | JWT tokens |
| `020-openid-oauth2` | OpenID Connect and OAuth2 |
| `021-authorization` | Authentication and authorization in practice |
| **Module 5 – Testing, observability, deployment** | |
| `022-testing` | Testing |
| `023-logging-monitoring` | Logging and monitoring |
| `024-containerization` | Deployment with containers |
