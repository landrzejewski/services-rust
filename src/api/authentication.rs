//! Authentication of API requests (step 018: HTTP Basic).

use std::sync::Arc;

use axum::{
    extract::{FromRef, FromRequestParts},
    http::request::Parts,
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Basic},
};
use secrecy::SecretString;
use uuid::Uuid;

use crate::{
    api::error::ApiError,
    domain::{
        auth_service::AuthService,
        error::DomainError,
        user::{Role, User},
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

impl From<User> for AuthUser {
    fn from(user: User) -> Self {
        Self {
            id: user.id,
            email: user.email.to_string(),
            role: user.role,
        }
    }
}

// Custom extractor reading only request parts (headers) -> `FromRequestParts`.
//
// `where Arc<AuthService>: FromRef<S>` – works with any router state that can provide the
// auth service (`AppState` via `#[derive(FromRef)]`).
impl<S> FromRequestParts<S> for AuthUser
where
    Arc<AuthService>: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // HTTP Basic: `Authorization: Basic base64(email:password)`.
        // `TypedHeader` parses and base64-decodes the header (crate `headers` via `axum-extra`).
        // Base64 is an encoding, NOT encryption – Basic auth is acceptable only over HTTPS.
        let TypedHeader(Authorization(basic)) =
            TypedHeader::<Authorization<Basic>>::from_request_parts(parts, state)
                .await
                // Missing or malformed header -> 401 (same as wrong credentials).
                .map_err(|_| ApiError::Domain(DomainError::Unauthenticated))?;

        let auth_service = Arc::<AuthService>::from_ref(state);
        let password = SecretString::from(basic.password());
        let user = auth_service
            .authenticate(basic.username(), &password)
            .await?;

        // Basic auth verifies the password (Argon2, ~tens of ms) on EVERY request – expensive
        // and the password travels with each call. Step 019 replaces it with signed tokens.
        Ok(user.into())
    }
}
