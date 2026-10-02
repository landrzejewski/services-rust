//! RFC 9457 "Problem Details for HTTP APIs" – a standard JSON format for error responses.
//!
//! ```json
//! HTTP/1.1 404 Not Found
//! Content-Type: application/problem+json
//!
//! { "type": "/problems/not-found", "title": "Resource not found", "status": 404,
//!   "detail": "room 0199... not found" }
//! ```

use std::collections::BTreeMap;

use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ProblemDetails {
    /// URI identifying the problem type (documentation link). Relative URIs are allowed.
    #[serde(rename = "type")]
    pub problem_type: String,
    /// Short, human-readable summary – the same for every occurrence of this type.
    pub title: String,
    /// HTTP status code, duplicated in the body for convenience.
    pub status: u16,
    /// Explanation specific to this occurrence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Extension member (allowed by the RFC): field-level validation messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<BTreeMap<String, Vec<String>>>,
}

impl ProblemDetails {
    pub fn new(status: StatusCode, slug: &str, title: &str) -> Self {
        Self {
            problem_type: format!("/problems/{slug}"),
            title: title.to_string(),
            status: status.as_u16(),
            detail: None,
            errors: None,
        }
    }

    // Builder-style setters taking and returning `self` allow chaining:
    // `ProblemDetails::new(..).with_detail(..).with_errors(..)`.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_errors(mut self, errors: BTreeMap<String, Vec<String>>) -> Self {
        self.errors = Some(errors);
        self
    }
}

impl IntoResponse for ProblemDetails {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut response = (status, Json(self)).into_response();
        // `Json` sets `application/json`; the RFC defines a dedicated media type.
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        response
    }
}
