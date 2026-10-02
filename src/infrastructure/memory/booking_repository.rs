use std::{collections::HashMap, sync::RwLock};

use chrono::{DateTime, Utc};
use uuid::Uuid;

use async_trait::async_trait;

use crate::domain::{
    booking::{Booking, BookingFilter, BookingStatus, NewBooking},
    repositories::{BookingRepository, RepositoryResult},
    time_range::TimeRange,
};

// Same structure as `InMemoryRoomRepository` – see comments there.
pub struct InMemoryBookingRepository {
    bookings: RwLock<HashMap<Uuid, Booking>>,
}

impl InMemoryBookingRepository {
    pub fn new() -> Self {
        Self {
            bookings: RwLock::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl BookingRepository for InMemoryBookingRepository {
    async fn insert(&self, new_booking: NewBooking) -> RepositoryResult<Booking> {
        let booking = Booking {
            id: Uuid::now_v7(),
            room_id: new_booking.room_id,
            user_id: new_booking.user_id,
            period: new_booking.period,
            attendees: new_booking.attendees,
            status: BookingStatus::Active,
            created_at: Utc::now(),
        };
        self.bookings
            .write()
            .expect("bookings lock poisoned")
            .insert(booking.id, booking.clone());
        Ok(booking)
    }

    async fn find_by_id(&self, id: Uuid) -> RepositoryResult<Option<Booking>> {
        Ok(self
            .bookings
            .read()
            .expect("bookings lock poisoned")
            .get(&id)
            .cloned())
    }

    async fn find(&self, filter: &BookingFilter) -> RepositoryResult<Vec<Booking>> {
        let bookings = self.bookings.read().expect("bookings lock poisoned");
        let mut result: Vec<Booking> = bookings
            .values()
            .filter(|booking| filter.matches(booking))
            .cloned()
            .collect();
        result.sort_by_key(|booking| booking.period.start());
        Ok(result)
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: BookingStatus,
    ) -> RepositoryResult<Option<Booking>> {
        let mut bookings = self.bookings.write().expect("bookings lock poisoned");
        let Some(booking) = bookings.get_mut(&id) else {
            return Ok(None);
        };
        booking.status = status;
        Ok(Some(booking.clone()))
    }

    async fn find_active_overlapping(
        &self,
        room_id: Uuid,
        period: &TimeRange,
    ) -> RepositoryResult<Vec<Booking>> {
        Ok(self
            .bookings
            .read()
            .expect("bookings lock poisoned")
            .values()
            .filter(|b| b.room_id == room_id && b.is_active() && b.period.overlaps(period))
            .cloned()
            .collect())
    }

    async fn count_active_by_user(
        &self,
        user_id: Uuid,
        from: DateTime<Utc>,
    ) -> RepositoryResult<usize> {
        Ok(self
            .bookings
            .read()
            .expect("bookings lock poisoned")
            .values()
            .filter(|b| b.user_id == user_id && b.is_active() && b.period.end() > from)
            .count())
    }

    async fn count_active_by_room(
        &self,
        room_id: Uuid,
        from: DateTime<Utc>,
    ) -> RepositoryResult<usize> {
        Ok(self
            .bookings
            .read()
            .expect("bookings lock poisoned")
            .values()
            .filter(|b| b.room_id == room_id && b.is_active() && b.period.end() > from)
            .count())
    }
}

impl Default for InMemoryBookingRepository {
    fn default() -> Self {
        Self::new()
    }
}
