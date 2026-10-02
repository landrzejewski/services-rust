//! Role-based access control in the API layer (step 021).
//!
//! Two ways to protect endpoints by role:
//! 1. route-level middleware (`require_admin` + `route_layer`) – one place guards a whole group
//!    of routes; handlers don't even mention it,
//! 2. an extractor (`AdminUser`) – the requirement is visible in the handler signature.
//!
//! Ownership ("only the owner may cancel") needs the resource and is checked in the domain
//! (`BookingService`, `Actor`).

use std::sync::Arc;

use axum::{
    extract::{FromRef, FromRequestParts, Request},
    http::request::Parts,
    middleware::Next,
    response::Response,
};

use crate::{
    api::{
        authentication::{AuthUser, Authenticator},
        error::ApiError,
    },
    domain::{
        error::DomainError,
        user::{Actor, Role},
    },
};

impl From<&AuthUser> for Actor {
    fn from(user: &AuthUser) -> Self {
        Actor {
            id: user.id,
            role: user.role,
        }
    }
}

fn forbidden() -> ApiError {
    ApiError::Domain(DomainError::Forbidden(
        "administrator role required".to_string(),
    ))
}

/// Middleware for `route_layer`: authentication (401) first, then the role check (403).
/// Extractors can be used as middleware arguments – here `AuthUser` does the authentication.
pub async fn require_admin(
    user: AuthUser,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    if user.role != Role::Admin {
        tracing::info!(user_id = %user.id, path = %request.uri().path(), "access denied");
        return Err(forbidden());
    }
    Ok(next.run(request).await)
}

/// Extractor variant: `admin: AdminUser` in a handler = "admins only".
pub struct AdminUser(pub AuthUser);

impl<S> FromRequestParts<S> for AdminUser
where
    Arc<Authenticator>: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // Reuse the authentication extractor, then add the authorization rule.
        let user = AuthUser::from_request_parts(parts, state).await?;
        if user.role == Role::Admin {
            Ok(Self(user))
        } else {
            Err(forbidden())
        }
    }
}
