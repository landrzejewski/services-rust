# 018 – Security fundamentals

## Goal

Understand the basic security goals and implement the first building blocks: users, safe password
storage, registration, login and authenticated requests (HTTP Basic).

## Key concepts

### Basic terms

| Term | Question | In this service |
|------|----------|-----------------|
| Identification | who do you claim to be? | e-mail |
| **Authentication** (AuthN) | prove it | password (018), token (019), identity provider (020) |
| **Authorization** (AuthZ) | what may you do? | roles USER/ADMIN, ownership of bookings (021) |
| **Confidentiality** | only intended parties can read data | TLS in transit, no secrets in logs, DTOs without internal fields |
| **Integrity** | data was not modified | TLS, signatures (JWT – 019), DB constraints |
| Availability | service keeps working | timeouts, body limits (011), rate limiting |
| Accountability | who did what | request ids, structured logs (023) |

HTTP status: **401** = not authenticated (missing/invalid credentials, must include `WWW-Authenticate`),
**403** = authenticated but not allowed (021).

### Storing passwords

Never store passwords or reversible encryptions of them. Store a **slow, salted, memory-hard hash**:

| Algorithm | Use |
|-----------|-----|
| **Argon2id** | recommended (OWASP), memory-hard |
| scrypt, bcrypt | acceptable alternatives |
| SHA-256, MD5 (even salted) | **never** for passwords – billions of guesses/s on GPUs |

```rust
let hash = Argon2::default().hash_password(password.as_bytes())?.to_string();
// $argon2id$v=19$m=19456,t=2,p=1$<salt>$<hash>   – algorithm, params and salt inside (PHC format)
let ok = Argon2::default().verify_password(password.as_bytes(), &PasswordHash::new(&hash)?).is_ok();
```

- random salt per password → identical passwords have different hashes, rainbow tables useless,
- parameters stored in the hash → can be increased later (rehash on next login),
- **CPU-heavy → `spawn_blocking`** (step 002),
- optional *pepper*: a secret key kept outside the database (`Argon2::new_with_secret`).

Password policy (NIST SP 800-63B): minimum length (here 12), maximum ≥ 64, no composition
rules, block known-breached passwords (e.g. HaveIBeenPwned k-anonymity API), no forced rotation.

### Login without information leaks

- same error for unknown e-mail and wrong password ("invalid credentials"),
- same **timing**: for unknown users verify against a dummy hash (`verify_dummy`),
- rate limiting / lockout against brute force (e.g. `tower_governor`, per IP + per account),
- registration reveals existing e-mails (409) – a UX trade-off; stricter: always "check your inbox".

### HTTP Basic authentication

```
Authorization: Basic base64("user@booking.local:user-password-123")
```

```rust
pub struct AuthUser { pub id: Uuid, pub email: String, pub role: Role }

impl<S> FromRequestParts<S> for AuthUser where Arc<AuthService>: FromRef<S>, S: Send + Sync {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        let TypedHeader(Authorization(basic)) =
            TypedHeader::<Authorization<Basic>>::from_request_parts(parts, state).await
                .map_err(|_| ApiError::Domain(DomainError::Unauthenticated))?;
        let user = Arc::<AuthService>::from_ref(state).authenticate(basic.username(), ...).await?;
        Ok(user.into())
    }
}

async fn create_booking(user: AuthUser, ValidatedJson(req): ValidatedJson<...>)   // protected endpoint
```

- base64 is encoding, not encryption → **only over HTTPS**,
- password sent and verified (Argon2) on every request → slow, risky; tokens solve this (019),
- fine for service-to-service or simple internal tools.

The owner of a booking now comes from `AuthUser`, not from the request body: never trust
client-supplied identity fields.

### Secrets in code and logs

- secrets come from env / secret managers (Vault, AWS/GCP secret managers, Kubernetes Secrets), never from git,
- `secrecy::SecretString` – `Debug` prints `[REDACTED]`, memory zeroed on drop, explicit `expose_secret()`,
- `SetSensitiveRequestHeadersLayer` – `Authorization`/`Cookie` values hidden in traces,
- DTOs never contain `password_hash` (008), `DatabaseSettings` masks its URL (014),
- error responses never include internals (010).

### Transport security (TLS)

- Production traffic must use HTTPS. Usually TLS terminates at a reverse proxy / load balancer /
  ingress (nginx, Traefik, cloud LB); the service listens on plain HTTP inside the private network.
- Direct TLS in Rust: `axum-server` with `rustls`, or `tokio-rustls` + `hyper-util`.
- HSTS header (`Strict-Transport-Security`) on the edge; database connections with `sslmode=require`/`verify-full`.

## What changed in this branch

- `migrations/*_create_users.sql` – `users` table, dev accounts, FK `bookings.user_id → users.id`
- `src/domain/user.rs` (new) – `User`, `Role`, `Email`, `Password` (policy, `SecretString`)
- `src/domain/password.rs` (new) – `PasswordHasher` port
- `src/domain/auth_service.rs` (new) – register, authenticate (no enumeration), tests
- `src/domain/error.rs` – `Unauthenticated`, `Internal`; `repositories.rs` – `UserRepository`
- `src/infrastructure/security/argon2_hasher.rs` (new) – Argon2id + `spawn_blocking`, test
- `src/infrastructure/{postgres,memory}/user_repository.rs` (new)
- `src/api/authentication.rs` (new) – `AuthUser` extractor (Basic)
- `src/api/auth.rs`, `dto/users.rs` (new) – `/auth/register`, `/auth/login`, `/users/me`, `/users/me/bookings`
- `src/api/bookings.rs`, `dto/bookings.rs` – owner from `AuthUser`, `userId` removed from the body
- `src/api/error.rs` – 401 + `WWW-Authenticate`; `middleware.rs` – sensitive headers
- `Cargo.toml` – `argon2` (optimized in dev), `axum-extra` (`typed-header`), `secrecy`

Dev accounts: `admin@booking.local` / `admin-password-123`, `user@booking.local` / `user-password-123`.

## Try it

```bash
cargo run
B=localhost:3000/api/v1
curl -X POST $B/auth/register -H 'content-type: application/json' -d '{"email":"anna@example.com","password":"anna-password-123"}'
curl -X POST $B/auth/login -H 'content-type: application/json' -d '{"email":"anna@example.com","password":"wrong-password"}'   # 401
curl -i $B/users/me                                                  # 401 + WWW-Authenticate
curl -u anna@example.com:anna-password-123 $B/users/me
curl -u user@booking.local:user-password-123 -X POST $B/bookings -H 'content-type: application/json' \
  -d '{"roomId":"0199a3f0-0000-7000-8000-000000000001","startTime":"2026-10-09T09:00:00Z","endTime":"2026-10-09T10:00:00Z"}'
docker compose exec postgres psql -U booking -d booking -c 'select email, password_hash from users'
```

## Exercises

1. Reject passwords contained in a small list of common passwords (`Password::parse`).
2. Add `rehash on login`: if the stored hash uses weaker parameters than the current ones, update it.
3. Add a per-IP rate limit for `/auth/login` with `tower_governor`.
