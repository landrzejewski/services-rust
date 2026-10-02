//! Authentication of API requests.
//! Step 018: HTTP Basic. Step 019: Bearer access tokens (JWT).

use std::sync::Arc;

use axum::{
    extract::{FromRef, FromRequestParts},
    http::request::Parts,
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};
use uuid::Uuid;

use crate::{
    api::error::ApiError,
    domain::{error::DomainError, user::Role},
    infrastructure::security::jwt::{JwtService, TokenError},
};

/// The authenticated caller. Adding `user: AuthUser` to a handler's arguments makes the
/// endpoint require authentication – the handler is never called without valid credentials.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: Uuid,
    pub email: String,
    pub role: Role,
}

// Custom extractor reading only request parts (headers) -> `FromRequestParts`.
//
// `where Arc<JwtService>: FromRef<S>` – works with any router state that can provide the
// token service (`AppState` via `#[derive(FromRef)]`).
impl<S> FromRequestParts<S> for AuthUser
where
    Arc<JwtService>: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // `Authorization: Bearer <token>` (RFC 6750). "Bearer" = whoever holds the token is
        // authenticated – protect tokens like passwords (HTTPS only, never in URLs or logs).
        let TypedHeader(Authorization(bearer)) =
            TypedHeader::<Authorization<Bearer>>::from_request_parts(parts, state)
                .await
                // No header / other scheme -> plain 401 (no `error=` attribute, RFC 6750 §3.1).
                .map_err(|_| ApiError::Domain(DomainError::Unauthenticated))?;

        // Stateless verification: signature + `exp` + `iss` + `aud`. No database access, no
        // password hashing – cheap enough for every request (unlike Basic auth in step 018).
        // Consequence: a token stays valid until it expires even if the user is deleted or the
        // role changes -> keep access tokens short-lived.
        let claims = Arc::<JwtService>::from_ref(state)
            .verify(bearer.token())
            .map_err(|error| {
                tracing::debug!(%error, "rejected access token");
                ApiError::InvalidToken(match error {
                    TokenError::Expired => "token expired".to_string(),
                    TokenError::Invalid(_) => "token invalid".to_string(),
                })
            })?;

        let role = claims
            .role()
            .ok_or_else(|| ApiError::InvalidToken("unknown role".to_string()))?;

        Ok(Self {
            id: claims.sub,
            email: claims.email,
            role,
        })
    }
}
