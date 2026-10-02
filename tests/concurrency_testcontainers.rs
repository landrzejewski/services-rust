//! Concurrency test of the booking transaction against a throw-away PostgreSQL container.
//!
//! testcontainers starts a real database in Docker for this test only (random host port) and
//! removes it afterwards – no shared state, no manual setup. Slower than other tests and needs
//! Docker, so it's `#[ignore]`d by default:
//!
//!     cargo test --test concurrency_testcontainers -- --ignored

mod common;

use std::sync::Arc;

use chrono::{TimeDelta, Utc};
use rust_services::{
    app::{self, Repositories},
    domain::{booking::NewBooking, error::DomainError, time_range::TimeRange, user::Email},
    infrastructure::postgres,
};
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ImageExt, runners::AsyncRunner},
};
use uuid::Uuid;

const HALL: Uuid = Uuid::from_u128(0x0199a3f0_0000_7000_8000_000000000003);

// `flavor = "multi_thread"` – the default test runtime is single-threaded; spawned tasks would
// only interleave at `.await`s. Several worker threads make the race realistic.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "starts a Docker container"]
async fn only_one_of_many_concurrent_bookings_for_the_same_slot_succeeds() {
    // Same image as in compose.yaml. The container lives as long as `container` is in scope.
    let container = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .expect("postgres container");
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");

    let mut settings = common::test_settings();
    settings.database.url = url;
    let pool = postgres::connect(&settings.database).await.expect("pool");
    postgres::run_migrations(&pool).await.expect("migrations");

    let repositories = Repositories::postgres(&settings, &pool).expect("repositories");
    let users = Arc::clone(&repositories.users);
    let state = app::build_state_with(&settings, repositories, pool).expect("state");

    // 10 different users (foreign key on bookings.user_id), all want the same slot.
    let start = (Utc::now() + TimeDelta::days(7))
        .date_naive()
        .and_hms_opt(10, 0, 0)
        .expect("time")
        .and_utc();
    let period = TimeRange::new(start, start + TimeDelta::hours(1)).expect("range");
    let mut tasks = Vec::new();
    for i in 0..10 {
        let user_id = Uuid::now_v7();
        let email = Email::parse(&format!("racer{i}@test.local")).expect("email");
        users
            .upsert_external(user_id, &email, rust_services::domain::user::Role::User)
            .await
            .expect("user");

        let service = Arc::clone(&state.booking_service);
        // `tokio::spawn` – truly parallel requests on the multi-threaded runtime below.
        tasks.push(tokio::spawn(async move {
            service
                .create_booking(NewBooking {
                    room_id: HALL,
                    user_id,
                    period,
                    attendees: 1,
                })
                .await
        }));
    }

    let mut created = 0;
    let mut conflicts = 0;
    for task in tasks {
        match task.await.expect("task") {
            Ok(_) => created += 1,
            Err(DomainError::Conflict(_)) => conflicts += 1,
            Err(other) => panic!("unexpected error: {other:?}"),
        }
    }
    assert_eq!((created, conflicts), (1, 9));
}
