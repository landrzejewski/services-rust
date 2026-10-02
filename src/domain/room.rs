//! Room – a bookable resource.
//!
//! Since step 008 domain types know nothing about JSON: no serde derives, no renames, no formats.
//! The API layer maps them to/from DTOs (`api::dto`). The domain can now change without breaking
//! the public API contract, and vice versa.

use std::fmt;

use chrono::NaiveTime;
use uuid::Uuid;

use crate::domain::validation::InvalidValue;

// A domain model: the shape of the data the business logic works with.
// Since step 009 it is built from validated value types (`RoomName`, `OpeningHours`),
// so an existing `Room` always satisfies its invariants.
#[derive(Debug, Clone)]
pub struct Room {
    pub id: Uuid,
    pub name: RoomName,
    pub description: Option<String>,
    pub capacity: u32,
    pub opening_hours: OpeningHours,
}

/// Data needed to create or replace a room (everything except the generated `id`).
/// A *command* object – input of a business operation.
#[derive(Debug, Clone)]
pub struct NewRoom {
    pub name: RoomName,
    pub description: Option<String>,
    pub capacity: u32,
    pub opening_hours: OpeningHours,
}

/// Newtype – a dedicated type wrapping a primitive. `RoomName` and `String` are different types,
/// so a non-validated string can't be passed where a room name is expected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomName(String);

impl RoomName {
    pub const MAX_LENGTH: usize = 100;

    /// The only constructor – normalizes (trims) and validates.
    pub fn parse(value: &str) -> Result<Self, InvalidValue> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(InvalidValue::new("name", "must not be blank"));
        }
        // `chars().count()` counts Unicode scalar values; `len()` would count bytes.
        if trimmed.chars().count() > Self::MAX_LENGTH {
            return Err(InvalidValue::new(
                "name",
                format!("must have at most {} characters", Self::MAX_LENGTH),
            ));
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// `Display` lets the name be used in `format!("{name}")` without exposing the inner `String`.
impl fmt::Display for RoomName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Daily opening hours, invariant: `opens_at < closes_at`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpeningHours {
    opens_at: NaiveTime,
    closes_at: NaiveTime,
}

impl OpeningHours {
    pub fn new(opens_at: NaiveTime, closes_at: NaiveTime) -> Result<Self, InvalidValue> {
        if opens_at >= closes_at {
            return Err(InvalidValue::new("closesAt", "must be after opensAt"));
        }
        Ok(Self {
            opens_at,
            closes_at,
        })
    }

    pub fn opens_at(&self) -> NaiveTime {
        self.opens_at
    }

    pub fn closes_at(&self) -> NaiveTime {
        self.closes_at
    }
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
            && self.name.as_ref().is_none_or(|name| {
                room.name
                    .as_str()
                    .to_lowercase()
                    .contains(&name.to_lowercase())
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_name_is_trimmed_and_validated() {
        assert_eq!(
            RoomName::parse("  Blue room ").unwrap().as_str(),
            "Blue room"
        );
        assert!(RoomName::parse("   ").is_err());
        assert!(RoomName::parse(&"x".repeat(101)).is_err());
    }

    #[test]
    fn opening_hours_must_be_ordered() {
        let eight = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
        let six_pm = NaiveTime::from_hms_opt(18, 0, 0).unwrap();

        assert!(OpeningHours::new(eight, six_pm).is_ok());
        assert!(OpeningHours::new(six_pm, eight).is_err());
    }
}
