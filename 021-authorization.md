# 021 – Authentication and authorization in practice

## Goal

Enforce who may do what: role-based rules for groups of endpoints, resource-based (ownership)
rules in the domain, correct 401/403 semantics.

## Key concepts

### Access rules of the service

| Endpoint | Anonymous | USER | ADMIN |
|----------|-----------|------|-------|
| `GET /rooms`, `GET /rooms/{id}` | ✔ | ✔ | ✔ |
| `POST/PUT/DELETE /rooms...` | 401 | 403 | ✔ |
| `GET /rooms/{id}/bookings`, `GET /bookings` | 401 | 403 | ✔ |
| `POST /bookings` | 401 | ✔ (for self) | ✔ (for self) |
| `GET /bookings/{id}`, `POST /bookings/{id}/cancel` | 401 | owner only (else 403) | ✔ |
| `GET /users/me`, `/users/me/bookings` | 401 | ✔ | ✔ |

### 401 vs 403 (vs 404)

| Status | Meaning | Client should |
|--------|---------|---------------|
| 401 Unauthorized | not authenticated / token invalid (+ `WWW-Authenticate`) | log in / refresh token |
| 403 Forbidden | authenticated, not allowed | nothing – retrying won't help |
| 404 Not Found | resource missing – or deliberately hiding that it exists | – |

Order of checks: authentication → authorization → business rules → validation of state.

### Models

| Model | Decision based on | Here |
|-------|-------------------|------|
| RBAC (role-based) | caller's role | USER / ADMIN |
| Ownership / ABAC (attribute-based) | attributes of caller + resource | `booking.user_id == caller.id` |
| ReBAC (relationship-based) | graph of relations (Zanzibar, OpenFGA, SpiceDB) | – |
| Scopes (OAuth) | what the *client* may do on the user's behalf | – (e.g. `bookings:write`) |

Roles come from the token (`role` claim / Keycloak `realm_access.roles`) – no DB lookup.

### Role guards in Axum

**Route-level middleware** – one guard for a group of routes:

```rust
pub async fn require_admin(user: AuthUser, request: Request, next: Next) -> Result<Response, ApiError> {
    if user.role != Role::Admin { return Err(forbidden()); }       // 403
    Ok(next.run(request).await)
}

let admin = Router::new()
    .route("/rooms", post(create_room))
    .route("/rooms/{id}", put(update_room).delete(delete_room))
    .route_layer(middleware::from_fn_with_state(state.clone(), require_admin));

public.merge(admin)       // same path, different methods: GET public, POST admin
```

- `AuthUser` as a middleware argument → 401 before the role check.
- `route_layer` runs only for matched routes → unknown URLs still return 404, not 401.
- `from_fn_with_state` – the middleware's extractors need the state (`Authenticator`).

**Extractor** – requirement visible in the signature:

```rust
pub struct AdminUser(pub AuthUser);
impl<S> FromRequestParts<S> for AdminUser { /* AuthUser + role check */ }

async fn list_bookings(_admin: AdminUser, ...) -> ...
```

Both are fine; be consistent. Middleware suits whole areas (`/admin/*`), extractors single endpoints.

### Ownership in the domain

The API can't decide "is this the owner?" without loading the booking → the domain decides:

```rust
pub struct Actor { pub id: Uuid, pub role: Role }         // domain view of the caller
impl Actor { pub fn can_manage(&self, owner_id: Uuid) -> bool { self.is_admin() || self.id == owner_id } }

pub async fn cancel_booking(&self, id: Uuid, actor: &Actor) -> DomainResult<Booking> {
    let mut booking = self.find_booking(id).await?;
    if !actor.can_manage(booking.user_id) { return Err(DomainError::Forbidden(...)); }
    ...
}
```

- `AuthUser` (API) → `Actor` (domain) via `From`; the domain stays independent of HTTP/JWT.
- Unit-testable: `only_owner_or_admin_can_cancel`.
- Identity fields (`userId`) never come from the request body (018).
- IDOR (Insecure Direct Object Reference): guessing another id must not grant access – every
  resource access checks ownership, regardless of UUIDs being hard to guess.

### Practical rules

- deny by default; open endpoints explicitly,
- check on the server for every request – hiding buttons in the UI is not authorization,
- log denied access (`tracing::info!`) for auditing; don't leak details in responses,
- test the matrix (step 022): every endpoint × anonymous / user / admin / other user.

## What changed in this branch

- `src/api/authorization.rs` (new) – `require_admin` middleware, `AdminUser` extractor, `AuthUser → Actor`
- `src/api/rooms.rs` – public vs admin routers, `route_layer`, `router(&AppState)`
- `src/api/bookings.rs` – admin-only list, owner checks via `Actor`
- `src/api/mod.rs` – access rules in the URL map
- `src/domain/user.rs` – `Actor`; `error.rs` – `Forbidden`; `api/error.rs` – 403 mapping
- `src/domain/booking_service.rs` – ownership checks for get/cancel, test

## Try it

```bash
cargo run
B=localhost:3000/api/v1
tok() { curl -s -X POST $B/auth/login -H 'content-type: application/json' -d "{\"email\":\"$1\",\"password\":\"$2\"}" | jq -r .accessToken; }
USER=$(tok user@booking.local user-password-123); ADMIN=$(tok admin@booking.local admin-password-123)
curl -i -X POST $B/rooms -H 'content-type: application/json' -d '{"name":"X","capacity":2}'                          # 401
curl -i -X POST $B/rooms -H "Authorization: Bearer $USER"  -H 'content-type: application/json' -d '{"name":"X","capacity":2}'   # 403
curl -i -X POST $B/rooms -H "Authorization: Bearer $ADMIN" -H 'content-type: application/json' -d '{"name":"X","capacity":2}'   # 201
curl -i $B/bookings -H "Authorization: Bearer $USER"                                                                   # 403
```

## Exercises

1. Add `GET /api/v1/rooms/{id}/availability?date=...` for all users: busy time slots without user ids.
2. Return 404 instead of 403 when a user accesses someone else's booking – adjust domain and tests.
3. Introduce a `MANAGER` role that may manage rooms but not see other users' bookings.
