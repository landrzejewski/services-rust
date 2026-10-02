use std::{collections::HashMap, sync::RwLock};

use chrono::Utc;
use uuid::Uuid;

use crate::domain::booking::{Booking, BookingFilter, BookingStatus, NewBooking};

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

    pub async fn insert(&self, new_booking: NewBooking) -> Booking {
        let booking = Booking {
            id: Uuid::now_v7(),
            room_id: new_booking.room_id,
            user_id: new_booking.user_id,
            start_time: new_booking.start_time,
            end_time: new_booking.end_time,
            attendees: new_booking.attendees,
            status: BookingStatus::Active,
            created_at: Utc::now(),
        };
        self.bookings
            .write()
            .expect("bookings lock poisoned")
            .insert(booking.id, booking.clone());
        booking
    }

    pub async fn find_by_id(&self, id: Uuid) -> Option<Booking> {
        self.bookings
            .read()
            .expect("bookings lock poisoned")
            .get(&id)
            .cloned()
    }

    pub async fn find(&self, filter: &BookingFilter) -> Vec<Booking> {
        let bookings = self.bookings.read().expect("bookings lock poisoned");
        let mut result: Vec<Booking> = bookings
            .values()
            .filter(|booking| filter.matches(booking))
            .cloned()
            .collect();
        result.sort_by_key(|booking| booking.start_time);
        result
    }

    pub async fn update_status(&self, id: Uuid, status: BookingStatus) -> Option<Booking> {
        let mut bookings = self.bookings.write().expect("bookings lock poisoned");
        let booking = bookings.get_mut(&id)?;
        booking.status = status;
        Some(booking.clone())
    }
}

impl Default for InMemoryBookingRepository {
    fn default() -> Self {
        Self::new()
    }
}
