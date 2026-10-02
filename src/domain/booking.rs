//! Booking – reservation of a room by a user for a time range.

use chrono::{DateTime, TimeDelta, Utc};
use uuid::Uuid;

use crate::domain::{error::DomainError, time_range::TimeRange};

// `DateTime<Utc>` (chrono) – an instant in time in UTC.
// Store and compute in UTC; convert to local time zones only for presentation.
#[derive(Debug, Clone)]
pub struct Booking {
    pub id: Uuid,
    pub room_id: Uuid,
    /// Users are not modelled yet – a plain id until authentication (step 018).
    pub user_id: Uuid,
    /// Validated time range (step 009) instead of two loose timestamps.
    pub period: TimeRange,
    pub attendees: u32,
    pub status: BookingStatus,
    pub created_at: DateTime<Utc>,
}

impl Booking {
    /// Derived value – computed from the state, not stored.
    pub fn duration(&self) -> TimeDelta {
        self.period.duration()
    }

    pub fn is_active(&self) -> bool {
        self.status == BookingStatus::Active
    }

    /// Behaviour on the entity: a state transition guarded by business rules.
    /// The entity protects its own consistency; the service only orchestrates.
    pub fn cancel(&mut self, now: DateTime<Utc>) -> Result<(), DomainError> {
        if !self.is_active() {
            return Err(DomainError::Conflict(format!(
                "booking {} is already cancelled",
                self.id
            )));
        }
        if self.period.start() <= now {
            return Err(DomainError::rule(
                "booking.already_started",
                "a booking that has already started cannot be cancelled",
            ));
        }
        self.status = BookingStatus::Cancelled;
        Ok(())
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
    pub period: TimeRange,
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
