use chrono::NaiveTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    api::serde_formats::hh_mm,
    domain::room::{NewRoom, Room, RoomFilter},
};

/// Body of `POST /rooms` and `PUT /rooms/{id}`.
//
// `rename_all = "camelCase"` – Rust fields stay snake_case, JSON uses the common JS convention:
// `opens_at` <-> `opensAt`. Other options: "snake_case", "kebab-case", "PascalCase",
// "SCREAMING_SNAKE_CASE", "lowercase".
//
// `deny_unknown_fields` – a typo like `"capasity"` fails with 422 instead of being silently ignored.
// Trade-off: clients can't send extra fields, so API evolution must be coordinated.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoomRequest {
    pub name: String,
    // `Option` fields are optional in JSON: absent or `null` -> `None`.
    pub description: Option<String>,
    pub capacity: u32,
    // `with = "module"` – custom (de)serialization for one field (see `api::serde_formats::hh_mm`).
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

// Request DTO -> domain command. Infallible for now; validation turns it into `TryFrom` (step 009).
impl From<RoomRequest> for NewRoom {
    fn from(request: RoomRequest) -> Self {
        Self {
            name: request.name,
            description: request.description,
            capacity: request.capacity,
            opens_at: request.opens_at,
            closes_at: request.closes_at,
        }
    }
}

/// Query string of `GET /rooms`: `?minCapacity=5&name=blue`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomQuery {
    pub min_capacity: Option<u32>,
    pub name: Option<String>,
}

impl From<RoomQuery> for RoomFilter {
    fn from(query: RoomQuery) -> Self {
        Self {
            min_capacity: query.min_capacity,
            name: query.name,
        }
    }
}

/// Room representation returned by the API.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomResponse {
    pub id: Uuid,
    pub name: String,
    // `skip_serializing_if` – omit the field from JSON when the predicate is true
    // (instead of emitting `"description": null`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub capacity: u32,
    #[serde(with = "hh_mm")]
    pub opens_at: NaiveTime,
    #[serde(with = "hh_mm")]
    pub closes_at: NaiveTime,
}

// Domain -> response DTO. Implementing `From` (not `Into`) is the convention:
// the standard library derives `Into` automatically from every `From` impl.
impl From<Room> for RoomResponse {
    fn from(room: Room) -> Self {
        Self {
            id: room.id,
            name: room.name,
            description: room.description,
            capacity: room.capacity,
            opens_at: room.opens_at,
            closes_at: room.closes_at,
        }
    }
}
