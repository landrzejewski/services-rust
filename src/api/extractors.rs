//! Custom extractors.
//!
//! The built-in `Path`, `Query` and `Json` reject invalid input with plain-text bodies.
//! The wrappers below behave the same, but their rejection is `ApiError`, so every error
//! of the API – including malformed input – is returned as Problem Details JSON.
//! Handlers import these instead of `axum::extract::{Path, Query}` / `axum::Json`.

use axum::{
    extract::{FromRequest, FromRequestParts, Request},
    response::{IntoResponse, Response},
};
use serde::{Serialize, de::DeserializeOwned};
use validator::Validate;

use crate::api::error::ApiError;

// `#[derive(FromRequestParts)]` (axum `macros` feature) implements the extractor by delegating:
// `via(axum::extract::Path)` – extract using the original extractor,
// `rejection(ApiError)`      – convert its rejection with `From<PathRejection> for ApiError`.
#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(ApiError))]
pub struct Path<T>(pub T);

#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Query), rejection(ApiError))]
pub struct Query<T>(pub T);

// `FromRequest` (not `...Parts`) – consumes the body.
#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(ApiError))]
pub struct Json<T>(pub T);

// The wrapper is also used for responses: delegate to `axum::Json`.
impl<T: Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        axum::Json(self.0).into_response()
    }
}

/// Like `Json<T>`, but additionally runs `T::validate()` before the handler is called.
///
/// ```ignore
/// async fn create(ValidatedJson(request): ValidatedJson<RoomRequest>) { /* request is valid */ }
/// ```
///
/// Handlers can't forget to validate, and invalid input never reaches business code.
pub struct ValidatedJson<T>(pub T);

// An extractor is any type implementing `FromRequest` (may consume the body) or
// `FromRequestParts` (headers, URI, extensions only). `S` is the router state type – this
// extractor works with any state, so it is generic over it.
impl<S, T> FromRequest<S> for ValidatedJson<T>
where
    // `DeserializeOwned` – deserializable without borrowing from the request body
    // (the body buffer is dropped after extraction).
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    // What is returned when extraction fails. Anything implementing `IntoResponse` works.
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        // Reuse `Json` for content-type checks and deserialization; `?` converts
        // its rejection (already an `ApiError`) and `ValidationErrors` via `From`.
        let Json(value) = Json::<T>::from_request(request, state).await?;
        value.validate()?;
        Ok(Self(value))
    }
}
