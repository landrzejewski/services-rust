//! Error returned when a value violates a domain invariant.

/// A value could not be turned into a valid domain type.
//
// `thiserror::Error` derive (step 010) generates the `Display` and `std::error::Error`
// impls that were written by hand in step 009. `#[error("...")]` is the `Display` format;
// fields are referenced by name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{field}: {message}")]
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
