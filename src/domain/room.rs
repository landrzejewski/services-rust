//! Room – a bookable resource.

use chrono::NaiveTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::serde_formats::hh_mm;

// A domain model: the shape of the data the business logic works with.
//
// `Serialize` is a shortcut for now – handlers return the domain model directly as JSON.
// That couples the API format to the domain; step 008 introduces separate DTOs.
//
// `rename_all = "camelCase"` – Rust fields stay snake_case, JSON uses the common JS convention:
// `opens_at` <-> `opensAt`. Other options: "snake_case", "kebab-case", "PascalCase",
// "SCREAMING_SNAKE_CASE", "lowercase".
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Room {
    // `Uuid` instead of a numeric id: globally unique, generated without a database round trip,
    // not guessable/enumerable. Serialized as a string "0199a3f0-....".
    pub id: Uuid,
    pub name: String,
    // `skip_serializing_if` – omit the field from JSON when the predicate is true
    // (instead of emitting `"description": null`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub capacity: u32,
    // `with = "module"` – custom (de)serialization for one field (see `serde_formats::hh_mm`).
    #[serde(with = "hh_mm")]
    pub opens_at: NaiveTime,
    #[serde(with = "hh_mm")]
    pub closes_at: NaiveTime,
}

/// Data needed to create or replace a room (everything except the generated `id`).
// `Deserialize` – the same shortcut in the other direction: request body -> domain type.
//
// `deny_unknown_fields` – a typo like `"capasity"` fails with 422 instead of being silently ignored.
// Trade-off: clients can't send extra fields, so API evolution must be coordinated.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewRoom {
    pub name: String,
    // `Option` fields are optional in JSON: absent or `null` -> `None`.
    pub description: Option<String>,
    pub capacity: u32,
    // `default = "fn"` – value used when the field is absent (plain `default` uses `Default::default()`).
    #[serde(with = "hh_mm", default = "default_opens_at")]
    pub opens_at: NaiveTime,
    #[serde(with = "hh_mm", default = "default_closes_at")]
    pub closes_at: NaiveTime,
}

fn default_opens_at() -> NaiveTime {
    NaiveTime::from_hms_opt(8, 0, 0).expect("valid constant time")
}

fn default_closes_at() -> NaiveTime {
    NaiveTime::from_hms_opt(18, 0, 0).expect("valid constant time")
}

/// Search criteria for rooms; every field is optional (`None` = no filtering).
// Query strings are deserialized by serde too, so the same attributes apply: `?minCapacity=5`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
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
