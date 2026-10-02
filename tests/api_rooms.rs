//! API tests for rooms: full stack (middleware, routing, extractors, handlers, services) on
//! in-memory storage. Run: `cargo test --test api_rooms`.

mod common;

use axum::http::{Method, StatusCode};
use common::TestApp;
use rust_services::domain::user::Role;
use serde_json::json;

// `#[tokio::test]` – each test gets its own runtime and its own `TestApp` (isolated state),
// so tests can run in parallel.
#[tokio::test]
async fn health_is_public() {
    let app = TestApp::new();

    let (status, body) = app.request(Method::GET, "/health/live", None, None).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "UP");
}

#[tokio::test]
async fn admin_creates_room_and_anyone_can_read_it() {
    let app = TestApp::new();
    let admin = app.token_for("admin@test.local", Role::Admin).await;

    let (status, created) = app
        .request(
            Method::POST,
            "/api/v1/rooms",
            Some(json!({ "name": "Test room", "capacity": 6, "opensAt": "09:00" })),
            Some(&admin),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["opensAt"], "09:00");
    assert_eq!(created["closesAt"], "18:00", "default from the DTO");

    let id = created["id"].as_str().expect("id");
    let (status, fetched) = app
        .request(Method::GET, &format!("/api/v1/rooms/{id}"), None, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["name"], "Test room");
}

#[tokio::test]
async fn managing_rooms_requires_admin_role() {
    let app = TestApp::new();
    let user = app.token_for("user@test.local", Role::User).await;
    let body = json!({ "name": "Nope", "capacity": 2 });

    let (anonymous, _) = app
        .request(Method::POST, "/api/v1/rooms", Some(body.clone()), None)
        .await;
    let (regular_user, problem) = app
        .request(Method::POST, "/api/v1/rooms", Some(body), Some(&user))
        .await;

    assert_eq!(anonymous, StatusCode::UNAUTHORIZED);
    assert_eq!(regular_user, StatusCode::FORBIDDEN);
    assert_eq!(problem["type"], "/problems/forbidden");
}

#[tokio::test]
async fn invalid_room_is_rejected_with_field_errors() {
    let app = TestApp::new();
    let admin = app.token_for("admin@test.local", Role::Admin).await;

    let (status, problem) = app
        .request(
            Method::POST,
            "/api/v1/rooms",
            Some(json!({ "name": "", "capacity": 0 })),
            Some(&admin),
        )
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["type"], "/problems/validation-error");
    assert!(problem["errors"]["name"].is_array());
    assert!(problem["errors"]["capacity"].is_array());
}

#[tokio::test]
async fn unknown_room_is_a_problem_details_404() {
    let app = TestApp::new();

    let (status, problem) = app
        .request(
            Method::GET,
            "/api/v1/rooms/0199a3f0-0000-7000-8000-00000000ffff",
            None,
            None,
        )
        .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(problem["status"], 404);
    // Added by the `enrich_problem_details` middleware (step 011).
    assert!(problem["requestId"].is_string());
}

#[tokio::test]
async fn rooms_are_paginated() {
    let app = TestApp::new();
    let admin = app.token_for("admin@test.local", Role::Admin).await;
    for i in 1..=3 {
        app.request(
            Method::POST,
            "/api/v1/rooms",
            Some(json!({ "name": format!("Room {i}"), "capacity": 4 })),
            Some(&admin),
        )
        .await;
    }

    let (status, page) = app
        .request(Method::GET, "/api/v1/rooms?page=2&size=2", None, None)
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["totalItems"], 3);
    assert_eq!(page["totalPages"], 2);
    assert_eq!(page["items"].as_array().map(Vec::len), Some(1));
}
