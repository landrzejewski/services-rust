use chrono::NaiveTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::{Validate, ValidationError};

use crate::{
    api::serde_formats::hh_mm,
    domain::{
        room::{NewRoom, OpeningHours, Room, RoomFilter, RoomName},
        validation::InvalidValue,
    },
};

/// Body of `POST /rooms` and `PUT /rooms/{id}`.
//
// `rename_all = "camelCase"` – Rust fields stay snake_case, JSON uses the common JS convention:
// `opens_at` <-> `opensAt`. Other options: "snake_case", "kebab-case", "PascalCase",
// "SCREAMING_SNAKE_CASE", "lowercase".
//
// `deny_unknown_fields` – a typo like `"capasity"` fails with 422 instead of being silently ignored.
// Trade-off: clients can't send extra fields, so API evolution must be coordinated.
//
// `Validate` (validator crate) generates `request.validate() -> Result<(), ValidationErrors>`
// from the `#[validate(...)]` attributes. All rules are checked and ALL failures are reported at
// once – clients get the complete list of problems in one response.
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
// Struct-level rule: a function receiving the whole struct, for rules involving several fields.
// Its errors are reported under the key `__all__`. By default it is skipped when any field rule
// failed; `skip_on_field_errors = false` runs it anyway, so all problems are reported at once.
#[validate(schema(function = "validate_opening_hours", skip_on_field_errors = false))]
pub struct RoomRequest {
    // `length` counts characters (not bytes). Custom `message` replaces the default error code text.
    #[validate(length(min = 1, max = 100, message = "must have 1-100 characters"))]
    pub name: String,
    // `Option` fields are optional in JSON: absent or `null` -> `None`.
    // Rules on `Option<T>` apply only when the value is present.
    #[validate(length(max = 500, message = "must have at most 500 characters"))]
    pub description: Option<String>,
    #[validate(range(min = 1, max = 500, message = "must be between 1 and 500"))]
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

fn validate_opening_hours(request: &RoomRequest) -> Result<(), ValidationError> {
    if request.opens_at >= request.closes_at {
        // The code ("opening_hours") is machine-readable; the message is for humans.
        return Err(ValidationError::new("opening_hours")
            .with_message("opensAt must be before closesAt".into()));
    }
    Ok(())
}

// Request DTO -> domain command. Fallible since step 009: building domain value types
// (`RoomName`, `OpeningHours`) can fail, so the mapping is `TryFrom` and returns `Result`.
// After `validate()` passed this should not fail – but the domain does not trust the API layer;
// it enforces its own invariants (the same domain types may be created from other inputs).
impl TryFrom<RoomRequest> for NewRoom {
    type Error = InvalidValue;

    fn try_from(request: RoomRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            name: RoomName::parse(&request.name)?,
            description: request.description,
            capacity: request.capacity,
            opening_hours: OpeningHours::new(request.opens_at, request.closes_at)?,
        })
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
            // Newtype -> primitive for the wire format.
            name: room.name.to_string(),
            description: room.description,
            capacity: room.capacity,
            opens_at: room.opening_hours.opens_at(),
            closes_at: room.opening_hours.closes_at(),
        }
    }
}
