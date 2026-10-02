//! Errors of business operations.

use uuid::Uuid;

use crate::domain::validation::InvalidValue;

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
}

/// Shorthand used by services: `DomainResult<Room>` = `Result<Room, DomainError>`.
pub type DomainResult<T> = Result<T, DomainError>;
