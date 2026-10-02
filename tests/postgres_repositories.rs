//! Repository tests against a real PostgreSQL (`#[sqlx::test]`).
//!
//! Requires a running server and `DATABASE_URL` (e.g. `docker compose up -d postgres` + `.env`).
//! For EVERY test sqlx creates a fresh, uniquely named database, applies `./migrations`
//! (including the seed data), passes a pool to the test and drops the database afterwards –
//! full isolation, tests run in parallel.

use chrono::{TimeDelta, TimeZone, Utc};
use rust_services::{
    domain::{
        booking::NewBooking,
        pagination::PageRequest,
        repositories::{BookingRepository, RepositoryError, RoomRepository, UserRepository},
        room::RoomFilter,
        time_range::TimeRange,
        user::{Email, Role},
    },
    infrastructure::postgres::{
        PostgresBookingRepository, PostgresRoomRepository, PostgresUserRepository,
    },
};
use sqlx::PgPool;
use uuid::Uuid;

const GREEN_ROOM: Uuid = Uuid::from_u128(0x0199a3f0_0000_7000_8000_000000000002);
const SEED_USER: Uuid = Uuid::from_u128(0x0199a3f0_0000_7000_8000_0000000000a2);

#[sqlx::test]
async fn filters_rooms_by_capacity_and_name(pool: PgPool) {
    let repository = PostgresRoomRepository::new(pool);
    let filter = RoomFilter {
        min_capacity: Some(5),
        name: Some("HALL".into()),
    };

    let page = repository
        .find(&filter, PageRequest::default())
        .await
        .unwrap();

    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].name.as_str(), "Conference hall");
}

fn booking(start_hour: u32, end_hour: u32) -> NewBooking {
    // Helpers outside `#[test]` functions are not covered by `allow-unwrap-in-tests` -> `expect`.
    let day = |h| {
        Utc.with_ymd_and_hms(2030, 1, 7, h, 0, 0)
            .single()
            .expect("valid date")
    };
    NewBooking {
        room_id: GREEN_ROOM,
        user_id: SEED_USER,
        period: TimeRange::new(day(start_hour), day(end_hour)).expect("valid range"),
        attendees: 1,
    }
}

/// The exclusion constraint (step 016) rejects overlaps even when application checks are
/// bypassed – here the repository is called directly, without the service and its locks.
#[sqlx::test]
async fn database_rejects_overlapping_bookings(pool: PgPool) {
    let repository = PostgresBookingRepository::new(pool);
    repository.insert(booking(9, 11)).await.unwrap();

    let overlapping = repository.insert(booking(10, 12)).await;
    let adjacent = repository.insert(booking(11, 12)).await;

    assert!(matches!(overlapping, Err(RepositoryError::Conflict(_))));
    assert!(adjacent.is_ok());
}

#[sqlx::test]
async fn counts_only_active_upcoming_bookings(pool: PgPool) {
    let repository = PostgresBookingRepository::new(pool);
    let created = repository.insert(booking(9, 10)).await.unwrap();
    let before = Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).unwrap();

    assert_eq!(
        repository
            .count_active_by_user(SEED_USER, before)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        repository
            .count_active_by_user(SEED_USER, before + TimeDelta::days(30))
            .await
            .unwrap(),
        0,
        "booking already ended"
    );

    repository
        .update_status(
            created.id,
            rust_services::domain::booking::BookingStatus::Cancelled,
        )
        .await
        .unwrap();
    assert_eq!(
        repository
            .count_active_by_user(SEED_USER, before)
            .await
            .unwrap(),
        0
    );
}

#[sqlx::test]
async fn upsert_external_user_creates_then_updates(pool: PgPool) {
    let repository = PostgresUserRepository::new(pool);
    let id = Uuid::now_v7();
    let email = Email::parse("external@idp.local").unwrap();

    repository
        .upsert_external(id, &email, Role::User)
        .await
        .unwrap();
    repository
        .upsert_external(id, &email, Role::Admin)
        .await
        .unwrap();

    let user = repository.find_by_id(id).await.unwrap().expect("user");
    assert_eq!(user.role, Role::Admin);
    assert!(user.password_hash.is_none());
}
