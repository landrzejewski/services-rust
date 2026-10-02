//! Room – a bookable resource.

use serde::Serialize;

// A domain model: the shape of the data the business logic works with.
//
// `Serialize` is a shortcut for now – handlers return the domain model directly as JSON.
// That couples the API format to the domain; step 008 introduces separate DTOs.
#[derive(Debug, Clone, Serialize)]
pub struct Room {
    pub id: u64,
    pub name: String,
    pub capacity: u32,
}
