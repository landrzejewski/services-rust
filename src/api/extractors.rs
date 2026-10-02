//! Custom extractors.

use std::collections::BTreeMap;

use axum::{
    Json,
    extract::{FromRequest, Request},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::de::DeserializeOwned;
use serde_json::json;
use validator::{Validate, ValidationErrors, ValidationErrorsKind};

use crate::domain::validation::InvalidValue;

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
    type Rejection = Response;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        // Reuse the standard `Json` extractor for content-type checks and deserialization,
        // only changing the rejection into a JSON body (default rejections are plain text).
        let Json(value) = Json::<T>::from_request(request, state)
            .await
            .map_err(|rejection| {
                let body = json!({ "error": rejection.body_text() });
                (rejection.status(), Json(body)).into_response()
            })?;

        value
            .validate()
            .map_err(|errors| validation_failed(field_messages(&errors)))?;

        Ok(Self(value))
    }
}

/// 422 response with messages grouped by field:
/// `{"error": "validation failed", "fields": {"name": ["must have 1-100 characters"]}}`
pub fn validation_failed(fields: BTreeMap<String, Vec<String>>) -> Response {
    let body = json!({ "error": "validation failed", "fields": fields });
    (StatusCode::UNPROCESSABLE_ENTITY, Json(body)).into_response()
}

/// Same response shape for domain invariant violations (`TryFrom` DTO -> domain type).
pub fn invalid_value(error: InvalidValue) -> Response {
    validation_failed(BTreeMap::from([(
        error.field.to_string(),
        vec![error.message],
    )]))
}

// `ValidationErrors` is a tree (nested structs / lists); flatten field-level messages.
// `BTreeMap` – sorted keys give a deterministic JSON output.
fn field_messages(errors: &ValidationErrors) -> BTreeMap<String, Vec<String>> {
    errors
        .errors()
        .iter()
        .filter_map(|(field, kind)| match kind {
            ValidationErrorsKind::Field(errors) => Some((
                field.to_string(),
                errors
                    .iter()
                    .map(|error| {
                        // Fall back to the error code when no custom message was set.
                        error
                            .message
                            .as_ref()
                            .map_or_else(|| error.code.to_string(), ToString::to_string)
                    })
                    .collect(),
            )),
            // Nested structs/lists are not used in this API.
            ValidationErrorsKind::Struct(_) | ValidationErrorsKind::List(_) => None,
        })
        .collect()
}
