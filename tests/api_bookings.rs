//! API tests for bookings and authentication flows.

mod common;

use axum::http::{Method, StatusCode};
use common::{TestApp, future_slot, random_email};
use rust_services::domain::user::Role;
use serde_json::{Value, json};

async fn create_room(app: &TestApp, admin: &str) -> String {
    let (status, room) = app
        .request(
            Method::POST,
            "/api/v1/rooms",
            Some(json!({ "name": "Booking room", "capacity": 4, "opensAt": "00:00", "closesAt": "23:59" })),
            Some(admin),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    room["id"].as_str().expect("id").to_string()
}

fn booking_body(room_id: &str, day: i64, hour: u32) -> Value {
    let (start, end) = future_slot(day, hour);
    json!({ "roomId": room_id, "startTime": start, "endTime": end })
}

#[tokio::test]
async fn booking_lifecycle_with_ownership_rules() {
    let app = TestApp::new();
    let admin = app.token_for("admin@test.local", Role::Admin).await;
    let owner = app.token_for("owner@test.local", Role::User).await;
    let stranger = app.token_for("stranger@test.local", Role::User).await;
    let room = create_room(&app, &admin).await;

    // Owner books a slot.
    let (status, booking) = app
        .request(
            Method::POST,
            "/api/v1/bookings",
            Some(booking_body(&room, 2, 10)),
            Some(&owner),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = booking["id"].as_str().expect("id").to_string();

    // The same slot for someone else -> 409.
    let (status, problem) = app
        .request(
            Method::POST,
            "/api/v1/bookings",
            Some(booking_body(&room, 2, 10)),
            Some(&stranger),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(problem["type"], "/problems/conflict");

    // The owner sees it in "my bookings".
    let (_, mine) = app
        .request(Method::GET, "/api/v1/users/me/bookings", None, Some(&owner))
        .await;
    assert_eq!(mine["totalItems"], 1);

    // A stranger may not cancel it, the owner may.
    let cancel = format!("/api/v1/bookings/{id}/cancel");
    let (status, _) = app
        .request(Method::POST, &cancel, None, Some(&stranger))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, cancelled) = app.request(Method::POST, &cancel, None, Some(&owner)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cancelled["status"], "CANCELLED");
}

#[tokio::test]
async fn business_rule_violation_returns_rule_id() {
    let app = TestApp::new();
    let admin = app.token_for("admin@test.local", Role::Admin).await;
    let user = app.token_for("user@test.local", Role::User).await;
    let room = create_room(&app, &admin).await;
    let mut body = booking_body(&room, 3, 9);
    body["attendees"] = json!(10);

    let (status, problem) = app
        .request(Method::POST, "/api/v1/bookings", Some(body), Some(&user))
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["rule"], "booking.capacity_exceeded");
}

/// Table-driven test of the access matrix (step 021): every row is one endpoint.
#[tokio::test]
async fn access_matrix() {
    let app = TestApp::new();
    let admin = app.token_for("admin@test.local", Role::Admin).await;
    let user = app.token_for("user@test.local", Role::User).await;
    let room = create_room(&app, &admin).await;

    // (method, path, expected anonymous, expected user, expected admin)
    let cases = [
        (Method::GET, "/api/v1/rooms".to_string(), 200, 200, 200),
        (
            Method::GET,
            format!("/api/v1/rooms/{room}/bookings"),
            401,
            403,
            200,
        ),
        (Method::GET, "/api/v1/bookings".to_string(), 401, 403, 200),
        (Method::GET, "/api/v1/users/me".to_string(), 401, 200, 200),
        (
            Method::GET,
            "/api/v1/users/me/bookings".to_string(),
            401,
            200,
            200,
        ),
    ];

    for (method, path, anonymous, as_user, as_admin) in cases {
        for (token, expected) in [
            (None, anonymous),
            (Some(&user), as_user),
            (Some(&admin), as_admin),
        ] {
            let (status, _) = app
                .request(method.clone(), &path, None, token.map(String::as_str))
                .await;
            assert_eq!(
                status.as_u16(),
                expected,
                "{method} {path} with token={}",
                token.is_some()
            );
        }
    }
}

/// End-to-end auth flow through HTTP: register -> login -> use the token (real Argon2 hashing).
#[tokio::test]
async fn register_login_and_call_protected_endpoint() {
    let app = TestApp::new();
    let email = random_email();
    let credentials = json!({ "email": email, "password": "integration-password" });

    let (status, _) = app
        .request(
            Method::POST,
            "/api/v1/auth/register",
            Some(credentials.clone()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, token) = app
        .request(Method::POST, "/api/v1/auth/login", Some(credentials), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(token["tokenType"], "Bearer");
    let access_token = token["accessToken"].as_str().expect("token");

    let (status, me) = app
        .request(Method::GET, "/api/v1/users/me", None, Some(access_token))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["email"], email);
    assert_eq!(me["role"], "USER");
}
