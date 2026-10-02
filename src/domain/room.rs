//! Room – a bookable resource.
//!
//! Since step 008 domain types know nothing about JSON: no serde derives, no renames, no formats.
//! The API layer maps them to/from DTOs (`api::dto`). The domain can now change without breaking
//! the public API contract, and vice versa.

use chrono::NaiveTime;
use uuid::Uuid;

// A domain model: the shape of the data the business logic works with.
#[derive(Debug, Clone)]
pub struct Room {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub capacity: u32,
    pub opens_at: NaiveTime,
    pub closes_at: NaiveTime,
}

/// Data needed to create or replace a room (everything except the generated `id`).
/// A *command* object – input of a business operation.
#[derive(Debug, Clone)]
pub struct NewRoom {
    pub name: String,
    pub description: Option<String>,
    pub capacity: u32,
    pub opens_at: NaiveTime,
    pub closes_at: NaiveTime,
}

/// Search criteria for rooms; every field is optional (`None` = no filtering).
#[derive(Debug, Clone, Default)]
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
