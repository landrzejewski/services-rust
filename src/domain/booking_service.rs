//! Business operations on bookings. Business rules (overlaps, opening hours, limits) – step 013.

use std::sync::Arc;

use uuid::Uuid;

use crate::domain::{
    booking::{Booking, BookingFilter, BookingStatus, NewBooking},
    error::{DomainError, DomainResult},
    repositories::{BookingRepository, RoomRepository},
    validation::InvalidValue,
};

pub struct BookingService {
    bookings: Arc<dyn BookingRepository>,
    // A service may use several repositories – here to check that the booked room exists.
    rooms: Arc<dyn RoomRepository>,
}

impl BookingService {
    pub fn new(bookings: Arc<dyn BookingRepository>, rooms: Arc<dyn RoomRepository>) -> Self {
        Self { bookings, rooms }
    }

    pub async fn create_booking(&self, new_booking: NewBooking) -> DomainResult<Booking> {
        // A missing *referenced* room is a problem of the input (field `roomId`), not of the URL –
        // reported as invalid value (422), not "not found" (404).
        if self.rooms.find_by_id(new_booking.room_id).await?.is_none() {
            return Err(InvalidValue::new("roomId", "room does not exist").into());
        }
        Ok(self.bookings.insert(new_booking).await?)
    }

    pub async fn get_booking(&self, id: Uuid) -> DomainResult<Booking> {
        self.bookings
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::booking_not_found(id))
    }

    pub async fn list_bookings(&self, filter: &BookingFilter) -> DomainResult<Vec<Booking>> {
        Ok(self.bookings.find(filter).await?)
    }

    /// Cancellation keeps the record (history) and only changes its status.
    pub async fn cancel_booking(&self, id: Uuid) -> DomainResult<Booking> {
        self.bookings
            .update_status(id, BookingStatus::Cancelled)
            .await?
            .ok_or_else(|| DomainError::booking_not_found(id))
    }
}
