//! Room – a bookable resource.

use serde::{Deserialize, Serialize};

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

/// Data needed to create or replace a room (everything except the generated `id`).
// `Deserialize` – the same shortcut in the other direction: request body -> domain type.
#[derive(Debug, Clone, Deserialize)]
pub struct NewRoom {
    pub name: String,
    pub capacity: u32,
}

/// Search criteria for rooms; every field is optional (`None` = no filtering).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RoomFilter {
    pub min_capacity: Option<u32>,
    pub name: Option<String>,
}

impl RoomFilter {
    pub fn matches(&self, room: &Room) -> bool {
        // `Option::is_none_or` – true when the filter is not set, otherwise evaluates the predicate.
        self.min_capacity.is_none_or(|min| room.capacity >= min)
            && self
                .name
                .as_ref()
                .is_none_or(|name| room.name.to_lowercase().contains(&name.to_lowercase()))
    }
}
