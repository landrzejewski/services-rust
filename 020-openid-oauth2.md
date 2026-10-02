# 020 – OpenID Connect and OAuth 2.0

## Goal

Delegate authentication to an identity provider (Keycloak): understand OAuth 2.0 roles and flows,
OpenID Connect additions, and validate provider-issued tokens in the API using discovery + JWKS.

## Key concepts

### OAuth 2.0 – delegated authorization

| Role | Here |
|------|------|
| Resource owner | the user (alice, bob) |
| Client | SPA / CLI (`room-booking-cli`) |
| Authorization server | Keycloak realm `room-booking` |
| Resource server | this API (audience `room-booking-api`) |

The client obtains an **access token** from the authorization server and sends it to the resource
server (`Authorization: Bearer ...`). The API never sees the user's password.

### Grant types (flows)

| Flow | For | Status |
|------|-----|--------|
| **Authorization Code + PKCE** | browser apps, mobile, CLI with browser | recommended for users |
| **Client Credentials** | service-to-service (no user) | recommended for machines |
| Refresh Token | renewing access tokens | used with the above |
| Device Authorization | TVs, CLIs without browser | ok |
| Resource Owner Password Credentials | app collects the password | legacy – removed in OAuth 2.1; used here only for curl demos |
| Implicit | old SPAs | deprecated |

Authorization Code + PKCE in short:

```
1. client → browser: /auth?response_type=code&client_id=...&redirect_uri=...&code_challenge=S256(verifier)&scope=openid
2. user logs in at Keycloak (password, MFA, SSO...)
3. Keycloak → redirect_uri?code=XYZ
4. client → POST /token  code=XYZ, code_verifier=verifier   (PKCE proves it's the same client)
5. ← { access_token, refresh_token, id_token }
```

### OpenID Connect – authentication layer on top of OAuth 2.0

- adds the **ID token** (JWT about the user, for the *client*) – the API uses the **access token**,
- `scope=openid` (+ `profile`, `email`), standard claims (`sub`, `email`, `preferred_username`),
- `userinfo` endpoint,
- **discovery**: `{issuer}/.well-known/openid-configuration` → endpoints, `jwks_uri`, supported algorithms,
- **JWKS**: provider's public keys (`kid`, `kty`, `n`, `e`), rotated over time.

### Validating provider tokens in the API

```
token header: {"alg":"RS256","kid":"KCEmD-..."}
1. discovery doc → jwks_uri (once, lazily)
2. JWKS → public key for kid (cache; unknown kid → refresh, rate-limited)
3. verify RS256 signature with the public key
4. check exp, iss == configured issuer, aud contains "room-booking-api"
5. map claims → AuthUser (sub, email, role from realm_access.roles)
```

```rust
let mut validation = Validation::new(Algorithm::RS256);       // pinned algorithm
validation.set_issuer(&[issuer]);
validation.set_audience(&["room-booking-api"]);
let key = DecodingKey::from_jwk(jwks.find(&kid)?)?;
let claims = decode::<OidcClaims>(token, &key, &validation)?.claims;
```

Always check `aud`: a token issued for another client/API of the same realm must not work here
(tested with an `admin-cli` token → 401). Keycloak adds the API to `aud` through an *audience mapper*
on the client.

### Two issuers side by side

| Token | alg | Verified with |
|-------|-----|---------------|
| local (`/auth/login`, step 019) | HS256 | shared secret |
| Keycloak | RS256 | provider public keys (JWKS) |

The `Authenticator` routes by header `alg`; each path pins its own algorithm and key, so the
header can't be used to switch verification keys.

### Users from the provider

The provider is the source of truth for identity. On the first request a local `users` row is
created/updated (`INSERT ... ON CONFLICT (id) DO UPDATE`) with `id = sub` and no password –
*just-in-time provisioning* – so bookings can reference the user. A process-local cache avoids a
write per request.

### Keycloak in compose

```yaml
keycloak:
  image: quay.io/keycloak/keycloak:26.4
  command: ["start-dev", "--import-realm"]
  environment:
    KC_BOOTSTRAP_ADMIN_USERNAME: admin
    KC_BOOTSTRAP_ADMIN_PASSWORD: admin
    KC_HOSTNAME: http://localhost:8180          # stable `iss`
    KC_HOSTNAME_BACKCHANNEL_DYNAMIC: "true"     # internal calls via keycloak:8080 (step 024)
  ports: ["8180:8080"]
  volumes: ["./keycloak:/opt/keycloak/data/import:ro"]
```

Realm `keycloak/room-booking-realm.json`: roles `USER`, `ADMIN`; clients `room-booking-api`
(audience) and `room-booking-cli` (public, PKCE, direct grants for demos); users
`alice`/`alice-password` (USER) and `bob`/`bob-password` (USER, ADMIN).
Admin console: http://localhost:8180 (admin/admin).

## What changed in this branch

- `compose.yaml` – `keycloak` service; `keycloak/room-booking-realm.json` (new)
- `src/infrastructure/security/oidc.rs` (new) – `OidcVerifier`: lazy discovery, JWKS cache, RS256 validation, role mapping
- `src/api/authentication.rs` – `Authenticator` (local HS256 + OIDC RS256), JIT provisioning
- `src/domain/auth_service.rs` – `ensure_external_user`; `repositories.rs` – `UserRepository::upsert_external`
- `src/infrastructure/{postgres,memory}/user_repository.rs` – upsert
- `src/config.rs`, `config/default.toml` – `[oidc]`
- `src/app.rs` – authenticator wiring; `Cargo.toml` – `reqwest` (rustls, json)

## Try it

```bash
docker compose up -d            # postgres + keycloak (first start takes ~30 s)
cargo run
KC=http://localhost:8180/realms/room-booking
curl -s $KC/.well-known/openid-configuration | jq '{issuer, jwks_uri, token_endpoint}'
curl -s $KC/protocol/openid-connect/certs | jq '.keys[] | {kid, alg, kty}'

TOKEN=$(curl -s -X POST $KC/protocol/openid-connect/token \
  -d grant_type=password -d client_id=room-booking-cli -d username=alice -d password=alice-password | jq -r .access_token)
curl -H "Authorization: Bearer $TOKEN" localhost:3000/api/v1/users/me
```

## Exercises

1. Create a confidential client with *service accounts enabled*, obtain a token with
   `grant_type=client_credentials` and decide how the API should treat tokens without a user e-mail.
2. Log in with Authorization Code + PKCE using `oauth2c` or Postman and call the API with the obtained token.
3. Disable a user in the Keycloak console – how long can they still call the API? How would you shorten that window?
