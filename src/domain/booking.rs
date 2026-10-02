//! Booking – reservation of a room by a user for a time range.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// `DateTime<Utc>` (chrono) – an instant in time in UTC. With chrono's `serde` feature it
// (de)serializes as an RFC 3339 string, e.g. "2026-10-05T09:00:00Z". On input any offset is
// accepted ("2026-10-05T11:00:00+02:00") and converted to UTC.
// Store and compute in UTC; convert to local time zones only for presentation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Booking {
    pub id: Uuid,
    pub room_id: Uuid,
    /// Users are not modelled yet – a plain id until authentication (step 018).
    pub user_id: Uuid,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub attendees: u32,
    pub status: BookingStatus,
    pub created_at: DateTime<Utc>,
}

// Unit-only enums serialize as strings. `rename_all` changes the variant names:
// `Active` -> "ACTIVE", `Cancelled` -> "CANCELLED". Works in JSON bodies and query strings.
// `PartialEq, Eq` allow comparing with `==`; `Copy` – the value is trivially copyable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BookingStatus {
    Active,
    Cancelled,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewBooking {
    pub room_id: Uuid,
    pub user_id: Uuid,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    // `default` – absent field -> `u32::default()`... which is 0. Here we want 1, hence a function.
    #[serde(default = "one")]
    pub attendees: u32,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookingFilter {
    pub room_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub status: Option<BookingStatus>,
}

impl BookingFilter {
    pub fn matches(&self, booking: &Booking) -> bool {
        self.room_id.is_none_or(|id| booking.room_id == id)
            && self.user_id.is_none_or(|id| booking.user_id == id)
            && self.status.is_none_or(|status| booking.status == status)
    }
}
