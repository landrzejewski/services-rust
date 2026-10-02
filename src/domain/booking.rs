//! Booking – reservation of a room by a user for a time range.

use chrono::{DateTime, TimeDelta, Utc};
use uuid::Uuid;

// `DateTime<Utc>` (chrono) – an instant in time in UTC.
// Store and compute in UTC; convert to local time zones only for presentation.
#[derive(Debug, Clone)]
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

impl Booking {
    /// Derived value – computed from the state, not stored.
    pub fn duration(&self) -> TimeDelta {
        self.end_time - self.start_time
    }
}

// `PartialEq, Eq` allow comparing with `==`; `Copy` – the value is trivially copyable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookingStatus {
    Active,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct NewBooking {
    pub room_id: Uuid,
    pub user_id: Uuid,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub attendees: u32,
}

#[derive(Debug, Clone, Default)]
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
