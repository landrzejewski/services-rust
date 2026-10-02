//! Booking – reservation of a room by a user for a time range.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// `DateTime<Utc>` (chrono) – an instant in time in UTC. With chrono's `serde` feature it
// (de)serializes as an RFC 3339 string, e.g. "2026-10-05T09:00:00Z". Details in step 007.
#[derive(Debug, Clone, Serialize)]
pub struct Booking {
    pub id: u64,
    pub room_id: u64,
    /// Users are not modelled yet – a plain id until authentication (step 018).
    pub user_id: u64,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub attendees: u32,
    pub status: BookingStatus,
}

// Enums with no data serialize as their variant name: "Active", "Cancelled".
// `PartialEq, Eq` allow comparing with `==`; `Copy` – the value is trivially copyable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BookingStatus {
    Active,
    Cancelled,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewBooking {
    pub room_id: u64,
    pub user_id: u64,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub attendees: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BookingFilter {
    pub room_id: Option<u64>,
    pub user_id: Option<u64>,
    pub status: Option<BookingStatus>,
}

impl BookingFilter {
    pub fn matches(&self, booking: &Booking) -> bool {
        self.room_id.is_none_or(|id| booking.room_id == id)
            && self.user_id.is_none_or(|id| booking.user_id == id)
            && self.status.is_none_or(|status| booking.status == status)
    }
}
