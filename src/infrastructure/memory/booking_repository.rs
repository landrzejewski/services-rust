use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use chrono::{DateTime, Utc};
use uuid::Uuid;

use async_trait::async_trait;

use super::transaction::in_memory;
use crate::domain::{
    booking::{Booking, BookingFilter, BookingStatus, NewBooking},
    pagination::{Page, PageRequest},
    repositories::{BookingRepository, RepositoryResult},
    time_range::TimeRange,
    transaction::Transaction,
};

// Same structure as `InMemoryRoomRepository` – see comments there.
pub struct InMemoryBookingRepository {
    bookings: Arc<RwLock<HashMap<Uuid, Booking>>>,
}

impl InMemoryBookingRepository {
    pub fn new() -> Self {
        Self {
            bookings: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

/// New active booking with generated id and timestamp – what the database does on INSERT.
fn build_booking(new_booking: NewBooking) -> Booking {
    Booking {
        id: Uuid::now_v7(),
        room_id: new_booking.room_id,
        user_id: new_booking.user_id,
        period: new_booking.period,
        attendees: new_booking.attendees,
        status: BookingStatus::Active,
        created_at: Utc::now(),
    }
}

#[async_trait]
impl BookingRepository for InMemoryBookingRepository {
    async fn find_by_id(&self, id: Uuid) -> RepositoryResult<Option<Booking>> {
        Ok(self
            .bookings
            .read()
            .expect("bookings lock poisoned")
            .get(&id)
            .cloned())
    }

    async fn find(
        &self,
        filter: &BookingFilter,
        page: PageRequest,
    ) -> RepositoryResult<Page<Booking>> {
        let bookings = self.bookings.read().expect("bookings lock poisoned");
        let mut result: Vec<Booking> = bookings
            .values()
            .filter(|booking| filter.matches(booking))
            .cloned()
            .collect();
        result.sort_by_key(|booking| booking.period.start());
        Ok(super::room_repository::paginate(result, page))
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

    // Transactional operations (step 016). Reads see committed data; the global lock of
    // `InMemoryTxManager` keeps them consistent until commit. `in_memory(tx)?` rejects
    // transactions of another storage.

    async fn insert(
        &self,
        tx: &mut dyn Transaction,
        new_booking: NewBooking,
    ) -> RepositoryResult<Booking> {
        let tx = in_memory(tx)?;
        let booking = build_booking(new_booking);
        let bookings = Arc::clone(&self.bookings);
        let stored = booking.clone();
        tx.defer(move || {
            bookings
                .write()
                .expect("bookings lock poisoned")
                .insert(stored.id, stored);
        });
        Ok(booking)
    }

    async fn lock_user(&self, tx: &mut dyn Transaction, _user_id: Uuid) -> RepositoryResult<()> {
        // The global lock already serializes everything.
        in_memory(tx)?;
        Ok(())
    }

    async fn find_active_overlapping(
        &self,
        tx: &mut dyn Transaction,
        room_id: Uuid,
        period: &TimeRange,
    ) -> RepositoryResult<Vec<Booking>> {
        in_memory(tx)?;
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
        tx: &mut dyn Transaction,
        user_id: Uuid,
        from: DateTime<Utc>,
    ) -> RepositoryResult<usize> {
        in_memory(tx)?;
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
        tx: &mut dyn Transaction,
        room_id: Uuid,
        from: DateTime<Utc>,
    ) -> RepositoryResult<usize> {
        in_memory(tx)?;
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
