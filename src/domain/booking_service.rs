//! Business operations on bookings. Business rules (overlaps, opening hours, limits) – step 013.

use std::sync::Arc;

use crate::{
    domain::booking::{Booking, BookingFilter, BookingStatus, NewBooking},
    infrastructure::memory::{InMemoryBookingRepository, InMemoryRoomRepository},
};

pub struct BookingService {
    bookings: Arc<InMemoryBookingRepository>,
    // A service may use several repositories – here to check that the booked room exists.
    rooms: Arc<InMemoryRoomRepository>,
}

impl BookingService {
    pub fn new(
        bookings: Arc<InMemoryBookingRepository>,
        rooms: Arc<InMemoryRoomRepository>,
    ) -> Self {
        Self { bookings, rooms }
    }

    /// `None` when the room does not exist (proper error types in step 010).
    pub async fn create_booking(&self, new_booking: NewBooking) -> Option<Booking> {
        // `?` on `Option` returns `None` early from the function.
        self.rooms.find_by_id(new_booking.room_id).await?;
        Some(self.bookings.insert(new_booking).await)
    }

    pub async fn get_booking(&self, id: u64) -> Option<Booking> {
        self.bookings.find_by_id(id).await
    }

    pub async fn list_bookings(&self, filter: &BookingFilter) -> Vec<Booking> {
        self.bookings.find(filter).await
    }

    /// Cancellation keeps the record (history) and only changes its status.
    pub async fn cancel_booking(&self, id: u64) -> Option<Booking> {
        self.bookings
            .update_status(id, BookingStatus::Cancelled)
            .await
    }
}
