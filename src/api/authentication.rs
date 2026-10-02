//! Authentication of API requests.
//! Step 018: HTTP Basic. Step 019: Bearer access tokens (JWT, HS256, issued by this service).
//! Step 020: additionally tokens from the OpenID Connect provider (Keycloak, RS256).

use std::sync::Arc;

use axum::{
    extract::{FromRef, FromRequestParts},
    http::request::Parts,
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};
use jsonwebtoken::{Algorithm, decode_header};
use uuid::Uuid;

use crate::{
    api::error::ApiError,
    domain::{auth_service::AuthService, error::DomainError, user::Role},
    infrastructure::security::{
        OidcVerifier,
        jwt::{JwtService, TokenError},
    },
};

/// The authenticated caller. Adding `user: AuthUser` to a handler's arguments makes the
/// endpoint require authentication – the handler is never called without valid credentials.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: Uuid,
    pub email: String,
    pub role: Role,
}

/// Verifies access tokens from all trusted issuers.
pub struct Authenticator {
    pub local: Arc<JwtService>,
    /// `None` when `oidc.enabled = false`.
    pub oidc: Option<Arc<OidcVerifier>>,
    pub auth_service: Arc<AuthService>,
}

impl Authenticator {
    async fn authenticate(&self, token: &str) -> Result<AuthUser, ApiError> {
        // Route by the algorithm in the (unverified) header. Safe because each verifier pins its
        // own algorithm and key: an HS256 token is checked only with our HMAC secret, an RS256
        // token only with the provider's public keys – a forged header can't switch keys.
        let header = decode_header(token).map_err(|_| invalid("token invalid"))?;
        match (header.alg, &self.oidc) {
            (Algorithm::HS256, _) => {
                let claims = self.local.verify(token).map_err(token_error)?;
                let role = claims.role().ok_or_else(|| invalid("unknown role"))?;
                Ok(AuthUser {
                    id: claims.sub,
                    email: claims.email,
                    role,
                })
            }
            (Algorithm::RS256, Some(oidc)) => {
                let claims = oidc.verify(token).await.map_err(token_error)?;
                let role = claims.role();
                let email = claims
                    .email
                    .clone()
                    .ok_or_else(|| invalid("token without email claim"))?;
                self.auth_service
                    .ensure_external_user(claims.sub, &email, role)
                    .await?;
                Ok(AuthUser {
                    id: claims.sub,
                    email,
                    role,
                })
            }
            _ => Err(invalid("unsupported token")),
        }
    }
}

fn invalid(reason: &str) -> ApiError {
    ApiError::InvalidToken(reason.to_string())
}

fn token_error(error: TokenError) -> ApiError {
    tracing::debug!(%error, "rejected access token");
    match error {
        TokenError::Expired => invalid("token expired"),
        TokenError::Invalid(_) => invalid("token invalid"),
    }
}

// Custom extractor reading only request parts (headers) -> `FromRequestParts`.
//
// `where Arc<Authenticator>: FromRef<S>` – works with any router state that can provide the
// authenticator (`AppState` via `#[derive(FromRef)]`).
impl<S> FromRequestParts<S> for AuthUser
where
    Arc<Authenticator>: FromRef<S>,
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

        // Stateless verification: signature + `exp` + `iss` + `aud`. Consequence: a token stays
        // valid until it expires even if the user is deleted or the role changes -> keep access
        // tokens short-lived.
        Arc::<Authenticator>::from_ref(state)
            .authenticate(bearer.token())
            .await
    }
}
