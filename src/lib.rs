//! Room Booking Service – library crate.
//!
//! Layered architecture (step 005). Dependencies point only downwards / inwards:
//!
//! ```text
//!   api             HTTP: routing, handlers, (de)serialization, status codes
//!    │ calls
//!   domain          business models and services – no HTTP, no SQL
//!    │ uses
//!   infrastructure  technical details: storage (in-memory now, PostgreSQL from step 014)
//!
//!   app / server    composition root: creates objects, wires layers, runs the HTTP server
//! ```
//!
//! Rules:
//! - `domain` never imports from `api` (no `axum` types in business code),
//! - handlers contain no business logic – they translate HTTP <-> domain calls,
//! - only the composition root (`app`) knows all concrete types.

pub mod api;
pub mod app;
pub mod config;
pub mod domain;
pub mod healthcheck;
pub mod infrastructure;
pub mod server;
pub mod telemetry;
