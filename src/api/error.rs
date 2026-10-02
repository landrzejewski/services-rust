//! API error type: everything a handler can fail with, and how it becomes an HTTP response.

use std::collections::BTreeMap;

use axum::{
    extract::rejection::{JsonRejection, PathRejection, QueryRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use validator::{ValidationErrors, ValidationErrorsKind};

use crate::{
    api::problem::ProblemDetails,
    domain::{error::DomainError, validation::InvalidValue},
};

/// Handlers return `ApiResult<T>`; `?` converts any error with a `From` impl into `ApiError`.
pub type ApiResult<T> = Result<T, ApiError>;

// `#[from]` generates `impl From<X> for ApiError`, which is what `?` uses.
// `#[error(transparent)]` forwards `Display` to the wrapped error.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error(transparent)]
    Domain(#[from] DomainError),

    #[error("request validation failed")]
    Validation(#[from] ValidationErrors),

    // Rejections of the built-in extractors (see `api::extractors`).
    #[error(transparent)]
    Json(#[from] JsonRejection),
    #[error(transparent)]
    Path(#[from] PathRejection),
    #[error(transparent)]
    Query(#[from] QueryRejection),

    #[error("no route for {0}")]
    RouteNotFound(String),
    #[error("method not allowed")]
    MethodNotAllowed,
}

// `?` performs ONE `From` conversion. `InvalidValue -> DomainError -> ApiError` would need two,
// so a direct impl lets handlers write `request.try_into()?` for DTO -> domain mapping.
impl From<InvalidValue> for ApiError {
    fn from(error: InvalidValue) -> Self {
        Self::Domain(error.into())
    }
}

// The single place mapping errors to status codes and response bodies.
// Handlers never build error responses by hand -> consistent API.
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let problem = match &self {
            ApiError::Domain(DomainError::NotFound { .. }) => {
                ProblemDetails::new(StatusCode::NOT_FOUND, "not-found", "Resource not found")
                    .with_detail(self.to_string())
            }
            ApiError::Domain(DomainError::Invalid(invalid)) => validation_problem(BTreeMap::from(
                [(invalid.field.to_string(), vec![invalid.message.clone()])],
            )),
            ApiError::Validation(errors) => validation_problem(field_messages(errors)),
            // Rejections already know their status (400/415/422) and message.
            ApiError::Json(rejection) => {
                ProblemDetails::new(rejection.status(), "invalid-body", "Invalid request body")
                    .with_detail(rejection.body_text())
            }
            ApiError::Path(rejection) => {
                ProblemDetails::new(rejection.status(), "invalid-path", "Invalid path parameter")
                    .with_detail(rejection.body_text())
            }
            ApiError::Query(rejection) => ProblemDetails::new(
                rejection.status(),
                "invalid-query",
                "Invalid query parameter",
            )
            .with_detail(rejection.body_text()),
            ApiError::RouteNotFound(_) => {
                ProblemDetails::new(StatusCode::NOT_FOUND, "not-found", "Route not found")
                    .with_detail(self.to_string())
            }
            ApiError::MethodNotAllowed => ProblemDetails::new(
                StatusCode::METHOD_NOT_ALLOWED,
                "method-not-allowed",
                "Method not allowed",
            ),
        };

        // Client errors are logged at `debug` – they are expected and can be noisy.
        // Server errors (5xx, from step 015) are logged at `error` with full details, while the
        // client gets only a generic message (no stack traces / SQL in responses).
        tracing::debug!(error = %self, status = problem.status, "request failed");
        problem.into_response()
    }
}

fn validation_problem(errors: BTreeMap<String, Vec<String>>) -> ProblemDetails {
    ProblemDetails::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation-error",
        "Validation failed",
    )
    .with_detail("one or more fields are invalid")
    .with_errors(errors)
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

#[cfg(test)]
mod tests {
    use axum::http::header;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn not_found_is_rendered_as_problem_details() {
        let response = ApiError::from(DomainError::room_not_found(Uuid::nil())).into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/problem+json"
        );
    }

    #[test]
    fn invalid_value_maps_to_422() {
        let error: ApiError = InvalidValue::new("name", "must not be blank").into();

        assert_eq!(
            error.into_response().status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
}
