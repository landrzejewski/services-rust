//! Business operations on bookings – the core business logic of the service (step 013).
//!
//! Rules for creating a booking:
//! 1. the room exists,
//! 2. attendees fit into the room (`capacity`),
//! 3. the booking starts in the future,
//! 4. it is not longer than the policy allows,
//! 5. it lies within the room's opening hours,
//! 6. the user doesn't exceed the limit of active bookings,
//! 7. it does not overlap another active booking of the same room.
//!
//! Rules for cancelling: only active bookings that haven't started yet (`Booking::cancel`).
//!
//! Step 016: creation runs in a transaction (`BookingUnitOfWork`) – locks + atomic
//! check-then-insert; the database exclusion constraint is the final safety net.

use std::sync::Arc;

use uuid::Uuid;

use crate::domain::{
    booking::{Booking, BookingFilter, NewBooking},
    booking_policy::BookingPolicy,
    clock::Clock,
    error::{DomainError, DomainResult},
    pagination::{Page, PageRequest},
    repositories::{BookingRepository, BookingTransaction, BookingUnitOfWork},
    room::Room,
    validation::InvalidValue,
};

pub struct BookingService {
    // Plain repository – reads and single-statement writes (list, get, cancel).
    bookings: Arc<dyn BookingRepository>,
    // Transactional operations for the multi-step "create booking" use case (step 016).
    unit_of_work: Arc<dyn BookingUnitOfWork>,
    clock: Arc<dyn Clock>,
    policy: BookingPolicy,
}

impl BookingService {
    pub fn new(
        bookings: Arc<dyn BookingRepository>,
        unit_of_work: Arc<dyn BookingUnitOfWork>,
        clock: Arc<dyn Clock>,
        policy: BookingPolicy,
    ) -> Self {
        Self {
            bookings,
            unit_of_work,
            clock,
            policy,
        }
    }

    pub async fn create_booking(&self, new_booking: NewBooking) -> DomainResult<Booking> {
        // BEGIN. From now on every `?` that returns early drops `tx` -> ROLLBACK.
        let mut tx = self.unit_of_work.begin().await?;

        // Lock the room row: concurrent bookings of the same room queue up here, so the
        // overlap check below sees all committed bookings and nobody can insert in between.
        // A missing *referenced* room is a problem of the input (field `roomId`), not of the URL –
        // reported as invalid value (422), not "not found" (404).
        let room = tx
            .lock_room(new_booking.room_id)
            .await?
            .ok_or_else(|| InvalidValue::new("roomId", "room does not exist"))?;

        // Cheap, data-independent checks first; checks requiring queries last.
        self.check_booking_fits_room(&new_booking, &room)?;

        // The per-user limit spans rooms, so the room lock doesn't protect it – lock the user too.
        // Lock order is always room -> user (consistent ordering prevents deadlocks).
        tx.lock_user(new_booking.user_id).await?;
        self.check_user_limit(tx.as_mut(), &new_booking).await?;
        self.check_no_overlap(tx.as_mut(), &new_booking).await?;

        let booking = tx.insert_booking(new_booking).await?;
        // COMMIT – releases the locks; only now other transactions see the new booking.
        tx.commit().await?;
        Ok(booking)
    }

    // Pure checks (no I/O) – easy to read and to test.
    fn check_booking_fits_room(&self, booking: &NewBooking, room: &Room) -> DomainResult<()> {
        if booking.attendees > room.capacity {
            return Err(DomainError::rule(
                "booking.capacity_exceeded",
                format!(
                    "{} attendees exceed the capacity of room '{}' ({})",
                    booking.attendees, room.name, room.capacity
                ),
            ));
        }
        if booking.period.start() <= self.clock.now() {
            return Err(DomainError::rule(
                "booking.start_in_past",
                "a booking must start in the future",
            ));
        }
        if booking.period.duration() > self.policy.max_duration {
            return Err(DomainError::rule(
                "booking.too_long",
                format!(
                    "a booking may last at most {} minutes",
                    self.policy.max_duration.num_minutes()
                ),
            ));
        }
        if !room.opening_hours.contains(&booking.period) {
            return Err(DomainError::rule(
                "booking.outside_opening_hours",
                format!(
                    "room '{}' is open {}-{} (UTC) and a booking must not span several days",
                    room.name,
                    room.opening_hours.opens_at().format("%H:%M"),
                    room.opening_hours.closes_at().format("%H:%M"),
                ),
            ));
        }
        Ok(())
    }

    // `&mut dyn BookingTransaction` – the checks run inside the caller's transaction.
    async fn check_user_limit(
        &self,
        tx: &mut dyn BookingTransaction,
        booking: &NewBooking,
    ) -> DomainResult<()> {
        let active = tx
            .count_active_by_user(booking.user_id, self.clock.now())
            .await?;
        if active >= self.policy.max_active_bookings_per_user {
            return Err(DomainError::rule(
                "booking.user_limit_reached",
                format!(
                    "a user may hold at most {} active bookings",
                    self.policy.max_active_bookings_per_user
                ),
            ));
        }
        Ok(())
    }

    async fn check_no_overlap(
        &self,
        tx: &mut dyn BookingTransaction,
        booking: &NewBooking,
    ) -> DomainResult<()> {
        let overlapping = tx
            .find_active_overlapping(booking.room_id, &booking.period)
            .await?;
        // Conflict (409), not a rule violation: the request is fine in itself, it collides
        // with existing state and may succeed for another time slot.
        if let Some(existing) = overlapping.first() {
            return Err(DomainError::Conflict(format!(
                "room is already booked from {} to {}",
                existing.period.start(),
                existing.period.end()
            )));
        }
        Ok(())
    }

    pub async fn get_booking(&self, id: Uuid) -> DomainResult<Booking> {
        self.bookings
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::booking_not_found(id))
    }

    pub async fn list_bookings(
        &self,
        filter: &BookingFilter,
        page: PageRequest,
    ) -> DomainResult<Page<Booking>> {
        Ok(self.bookings.find(filter, page).await?)
    }

    /// Cancellation keeps the record (history) and only changes its status.
    /// Typical flow of a state change: load -> call behaviour on the entity -> persist.
    pub async fn cancel_booking(&self, id: Uuid) -> DomainResult<Booking> {
        let mut booking = self.get_booking(id).await?;
        booking.cancel(self.clock.now())?;
        self.bookings
            .update_status(id, booking.status)
            .await?
            .ok_or_else(|| DomainError::booking_not_found(id))
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, NaiveTime, TimeZone, Utc};

    use super::*;
    use crate::{
        domain::{
            clock::FixedClock,
            repositories::RoomRepository,
            room::{NewRoom, OpeningHours, RoomName},
            time_range::TimeRange,
        },
        // Tests reuse the in-memory adapters as lightweight fakes.
        infrastructure::memory::{
            InMemoryBookingRepository, InMemoryBookingUnitOfWork, InMemoryRoomRepository,
        },
    };

    // "Now" in all tests: Monday 2026-10-05 07:00 UTC.
    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 5, 7, 0, 0).unwrap()
    }

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 5, hour, 0, 0).unwrap()
    }

    // Test fixture: service with a fixed clock and one room (capacity 4, open 08:00-18:00).
    async fn setup() -> (BookingService, Uuid) {
        let rooms = Arc::new(InMemoryRoomRepository::new());
        let bookings = Arc::new(InMemoryBookingRepository::new());
        let hour = |h| NaiveTime::from_hms_opt(h, 0, 0).unwrap();
        let room = rooms
            .insert(NewRoom {
                name: RoomName::parse("Test room").unwrap(),
                description: None,
                capacity: 4,
                opening_hours: OpeningHours::new(hour(8), hour(18)).unwrap(),
            })
            .await
            .unwrap();
        let service = BookingService::new(
            Arc::clone(&bookings) as Arc<dyn BookingRepository>,
            Arc::new(InMemoryBookingUnitOfWork::new(rooms, bookings)),
            Arc::new(FixedClock(now())),
            BookingPolicy {
                max_active_bookings_per_user: 2,
                ..BookingPolicy::default()
            },
        );
        (service, room.id)
    }

    fn booking(room_id: Uuid, user_id: Uuid, from: u32, to: u32, attendees: u32) -> NewBooking {
        NewBooking {
            room_id,
            user_id,
            period: TimeRange::new(at(from), at(to)).unwrap(),
            attendees,
        }
    }

    // `matches!` checks the error variant (and here the rule id) without comparing messages.
    fn assert_rule(result: DomainResult<Booking>, expected: &str) {
        assert!(
            matches!(&result, Err(DomainError::RuleViolated { rule, .. }) if *rule == expected),
            "expected rule {expected}, got {result:?}"
        );
    }

    #[tokio::test]
    async fn creates_valid_booking() {
        let (service, room) = setup().await;

        let created = service
            .create_booking(booking(room, Uuid::now_v7(), 9, 10, 3))
            .await;

        assert!(created.is_ok());
    }

    #[tokio::test]
    async fn rejects_too_many_attendees() {
        let (service, room) = setup().await;

        let result = service
            .create_booking(booking(room, Uuid::now_v7(), 9, 10, 5))
            .await;

        assert_rule(result, "booking.capacity_exceeded");
    }

    #[tokio::test]
    async fn rejects_booking_in_the_past() {
        let (service, room) = setup().await;

        let result = service
            .create_booking(booking(room, Uuid::now_v7(), 6, 8, 1))
            .await;

        assert_rule(result, "booking.start_in_past");
    }

    #[tokio::test]
    async fn rejects_booking_outside_opening_hours() {
        let (service, room) = setup().await;

        let result = service
            .create_booking(booking(room, Uuid::now_v7(), 17, 19, 1))
            .await;

        assert_rule(result, "booking.outside_opening_hours");
    }

    #[tokio::test]
    async fn rejects_overlapping_booking_but_allows_adjacent() {
        let (service, room) = setup().await;
        service
            .create_booking(booking(room, Uuid::now_v7(), 9, 11, 1))
            .await
            .unwrap();

        let overlapping = service
            .create_booking(booking(room, Uuid::now_v7(), 10, 12, 1))
            .await;
        let adjacent = service
            .create_booking(booking(room, Uuid::now_v7(), 11, 12, 1))
            .await;

        assert!(matches!(overlapping, Err(DomainError::Conflict(_))));
        assert!(adjacent.is_ok());
    }

    #[tokio::test]
    async fn enforces_user_limit() {
        let (service, room) = setup().await;
        let user = Uuid::now_v7();
        service
            .create_booking(booking(room, user, 9, 10, 1))
            .await
            .unwrap();
        service
            .create_booking(booking(room, user, 10, 11, 1))
            .await
            .unwrap();

        let third = service.create_booking(booking(room, user, 11, 12, 1)).await;

        assert_rule(third, "booking.user_limit_reached");
    }

    #[tokio::test]
    async fn cancelled_booking_frees_the_slot_and_cannot_be_cancelled_twice() {
        let (service, room) = setup().await;
        let first = service
            .create_booking(booking(room, Uuid::now_v7(), 9, 10, 1))
            .await
            .unwrap();

        service.cancel_booking(first.id).await.unwrap();
        let again = service.cancel_booking(first.id).await;
        let same_slot = service
            .create_booking(booking(room, Uuid::now_v7(), 9, 10, 1))
            .await;

        assert!(matches!(again, Err(DomainError::Conflict(_))));
        assert!(same_slot.is_ok());
    }
}
