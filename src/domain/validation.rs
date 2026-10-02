//! Error returned when a value violates a domain invariant.

use std::fmt;

/// A value could not be turned into a valid domain type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidValue {
    /// Name of the offending field (used by the API layer to build field-level error responses).
    pub field: &'static str,
    pub message: String,
}

impl InvalidValue {
    pub fn new(field: &'static str, message: impl Into<String>) -> Self {
        Self {
            field,
            message: message.into(),
        }
    }
}

// `Display` + `std::error::Error` make it a regular Rust error type (usable with `?`,
// `Box<dyn Error>`, logging). Step 010 replaces this boilerplate with the `thiserror` derive.
impl fmt::Display for InvalidValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

impl std::error::Error for InvalidValue {}
