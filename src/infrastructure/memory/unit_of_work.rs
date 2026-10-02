//! In-memory unit of work – for unit tests of the booking rules (step 016).

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::{Mutex, OwnedMutexGuard};
use uuid::Uuid;

use super::{InMemoryBookingRepository, InMemoryRoomRepository, booking_repository::build_booking};
use crate::domain::{
    booking::{Booking, NewBooking},
    repositories::{
        BookingRepository, BookingTransaction, BookingUnitOfWork, RepositoryResult, RoomRepository,
    },
    room::Room,
    time_range::TimeRange,
};

/// Isolation by brute force: one global async mutex – transactions run strictly one after another.
/// Inserts are buffered and applied on commit, so a dropped transaction leaves no trace.
pub struct InMemoryBookingUnitOfWork {
    rooms: Arc<InMemoryRoomRepository>,
    bookings: Arc<InMemoryBookingRepository>,
    lock: Arc<Mutex<()>>,
}

impl InMemoryBookingUnitOfWork {
    pub fn new(
        rooms: Arc<InMemoryRoomRepository>,
        bookings: Arc<InMemoryBookingRepository>,
    ) -> Self {
        Self {
            rooms,
            bookings,
            lock: Arc::new(Mutex::new(())),
        }
    }
}

#[async_trait]
impl BookingUnitOfWork for InMemoryBookingUnitOfWork {
    async fn begin(&self) -> RepositoryResult<Box<dyn BookingTransaction>> {
        // `tokio::sync::Mutex` (not `std`): the guard lives across `.await` points for the whole
        // transaction. `lock_owned` returns a guard that owns an `Arc` to the mutex, so it can be
        // stored in a struct with no borrowed lifetime.
        let guard = Arc::clone(&self.lock).lock_owned().await;
        Ok(Box::new(InMemoryBookingTransaction {
            _guard: guard,
            rooms: Arc::clone(&self.rooms),
            bookings: Arc::clone(&self.bookings),
            pending: Vec::new(),
        }))
    }
}

struct InMemoryBookingTransaction {
    // Held until the transaction is committed or dropped; never read (hence `_`).
    _guard: OwnedMutexGuard<()>,
    rooms: Arc<InMemoryRoomRepository>,
    bookings: Arc<InMemoryBookingRepository>,
    pending: Vec<Booking>,
}

#[async_trait]
impl BookingTransaction for InMemoryBookingTransaction {
    async fn lock_room(&mut self, room_id: Uuid) -> RepositoryResult<Option<Room>> {
        // The global lock already serializes everything.
        self.rooms.find_by_id(room_id).await
    }

    async fn lock_user(&mut self, _user_id: Uuid) -> RepositoryResult<()> {
        Ok(())
    }

    async fn find_active_overlapping(
        &mut self,
        room_id: Uuid,
        period: &TimeRange,
    ) -> RepositoryResult<Vec<Booking>> {
        let mut result = self
            .bookings
            .find_active_overlapping(room_id, period)
            .await?;
        // "Read your own writes" – uncommitted inserts of this transaction are visible to it.
        result.extend(
            self.pending
                .iter()
                .filter(|b| b.room_id == room_id && b.period.overlaps(period))
                .cloned(),
        );
        Ok(result)
    }

    async fn count_active_by_user(
        &mut self,
        user_id: Uuid,
        from: DateTime<Utc>,
    ) -> RepositoryResult<usize> {
        let stored = self.bookings.count_active_by_user(user_id, from).await?;
        let pending = self
            .pending
            .iter()
            .filter(|b| b.user_id == user_id && b.period.end() > from)
            .count();
        Ok(stored + pending)
    }

    async fn insert_booking(&mut self, new_booking: NewBooking) -> RepositoryResult<Booking> {
        let booking = build_booking(new_booking);
        self.pending.push(booking.clone());
        Ok(booking)
    }

    async fn commit(self: Box<Self>) -> RepositoryResult<()> {
        for booking in self.pending {
            self.bookings.put(booking);
        }
        Ok(())
        // `self` (with the guard) is dropped here -> the next transaction may start.
    }
}
