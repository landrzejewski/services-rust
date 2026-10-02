//! Errors of business operations.

use uuid::Uuid;

use crate::domain::{repositories::RepositoryError, validation::InvalidValue};

/// Everything a domain service can report to its caller.
///
/// Variants describe *what went wrong in business terms* – not HTTP status codes.
/// The API layer decides how each variant is presented (`api::error`), another adapter
/// (CLI, message consumer) could present them differently.
//
// `thiserror` generates `Display` from `#[error]` and `From` impls from `#[from]`,
// so `?` converts e.g. `InvalidValue` into `DomainError::Invalid` automatically.
#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("{entity} {id} not found")]
    NotFound { entity: &'static str, id: Uuid },

    #[error(transparent)]
    Invalid(#[from] InvalidValue),

    /// A business rule rejected the operation (step 013). `rule` is a stable, machine-readable
    /// identifier clients can react to; `message` explains it to humans.
    #[error("{message}")]
    RuleViolated { rule: &'static str, message: String },

    /// The operation conflicts with the current state of a resource (overlap, already cancelled).
    #[error("{0}")]
    Conflict(String),

    /// Technical failure below the domain – not the client's fault (-> 500).
    // No `#[from]` since step 015: the conversion is written by hand below,
    // because one repository variant maps to a *different* domain variant.
    #[error(transparent)]
    Repository(RepositoryError),
}

// `?` on `RepositoryResult` inside services uses this impl.
impl From<RepositoryError> for DomainError {
    fn from(error: RepositoryError) -> Self {
        match error {
            RepositoryError::Conflict(message) => DomainError::Conflict(message),
            other => DomainError::Repository(other),
        }
    }
}

impl DomainError {
    // Small constructors keep call sites short: `DomainError::room_not_found(id)`.
    pub fn room_not_found(id: Uuid) -> Self {
        Self::NotFound { entity: "room", id }
    }

    pub fn booking_not_found(id: Uuid) -> Self {
        Self::NotFound {
            entity: "booking",
            id,
        }
    }

    pub fn rule(rule: &'static str, message: impl Into<String>) -> Self {
        Self::RuleViolated {
            rule,
            message: message.into(),
        }
    }
}

/// Shorthand used by services: `DomainResult<Room>` = `Result<Room, DomainError>`.
pub type DomainResult<T> = Result<T, DomainError>;
