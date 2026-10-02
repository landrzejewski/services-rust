//! Configurable parameters of the booking rules.

use chrono::TimeDelta;

/// Limits that are business decisions, not code – loaded from configuration (`[booking]`),
/// passed to `BookingService`.
#[derive(Debug, Clone)]
pub struct BookingPolicy {
    /// How many upcoming active bookings a single user may hold.
    pub max_active_bookings_per_user: usize,
    /// Longest allowed booking.
    pub max_duration: TimeDelta,
}

impl Default for BookingPolicy {
    fn default() -> Self {
        Self {
            max_active_bookings_per_user: 3,
            max_duration: TimeDelta::hours(8),
        }
    }
}
