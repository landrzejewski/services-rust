//! Shared helpers for integration tests.
//!
//! Every file in `tests/` is compiled as a separate crate that uses the library like an external
//! user (`rust_services::...`). Code in `tests/common/mod.rs` is NOT a test target itself;
//! test files include it with `mod common;`.

// Not every test file uses every helper.
#![allow(dead_code)]

use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use chrono::Utc;
use rust_services::{
    app::{self, AppState, Repositories},
    config::Settings,
    domain::{
        repositories::UserRepository,
        user::{Email, NewUser, Role},
    },
};
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use uuid::Uuid;

/// The complete application (router + middleware + services) on in-memory storage.
pub struct TestApp {
    pub router: Router,
    pub state: AppState,
    users: Arc<dyn UserRepository>,
}

pub fn test_settings() -> Settings {
    // Real config files + overrides: tests don't depend on `.env` or the shell environment.
    Settings::load_with_overrides(&[
        (
            "auth.jwt_secret",
            "integration-test-secret-at-least-32-bytes",
        ),
        ("oidc.enabled", "false"),
        // Never used: `connect_lazy` below doesn't open connections.
        (
            "database.url",
            "postgres://unused:unused@localhost:1/unused",
        ),
    ])
    .expect("test settings")
}

impl TestApp {
    pub fn new() -> Self {
        let settings = test_settings();
        let repositories = Repositories::in_memory();
        let users = Arc::clone(&repositories.users);
        // `AppState` requires a pool (readiness probe). A lazy pool connects only when used.
        let db = PgPoolOptions::new()
            .connect_lazy(&settings.database.url)
            .expect("lazy pool");
        let state = app::build_state_with(&settings, repositories, db).expect("state");
        let router = app::build_router(state.clone(), &settings);
        Self {
            router,
            state,
            users,
        }
    }

    /// Creates a user directly in the repository and returns an access token for it –
    /// faster than register + login through HTTP in every test.
    pub async fn token_for(&self, email: &str, role: Role) -> String {
        let user = self
            .users
            .insert(NewUser {
                email: Email::parse(email).expect("valid email"),
                password_hash: None,
                role,
            })
            .await
            .expect("user created");
        self.state.jwt.issue(&user).expect("token").token
    }

    /// Sends one request through the full stack (middleware, routing, extractors, handlers).
    ///
    /// `ServiceExt::oneshot` (tower) calls the router as a `Service` – no TCP socket, no port,
    /// no running server; tests are fast and can run in parallel.
    pub async fn request(
        &self,
        method: Method,
        uri: &str,
        body: Option<Value>,
        token: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let request = match body {
            Some(json) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json.to_string())),
            None => builder.body(Body::empty()),
        }
        .expect("request");

        // `Router` is cloned because `oneshot` consumes the service.
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("infallible");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let json = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, json)
    }
}

/// An instant safely in the future (bookings must start in the future).
pub fn future_slot(day_offset: i64, hour: u32) -> (String, String) {
    let day = (Utc::now() + chrono::TimeDelta::days(day_offset)).date_naive();
    let start = day.and_hms_opt(hour, 0, 0).expect("valid hour").and_utc();
    let end = start + chrono::TimeDelta::hours(1);
    (start.to_rfc3339(), end.to_rfc3339())
}

pub fn random_email() -> String {
    format!("user-{}@test.local", Uuid::now_v7())
}
