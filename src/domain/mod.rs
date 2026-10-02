//! Domain layer: business models and business operations.
//!
//! Must not depend on `axum`, HTTP status codes, SQL or any other delivery/storage detail.
//! It can be unit-tested without a server or a database.

pub mod booking;
pub mod booking_service;
pub mod error;
pub mod repositories;
pub mod room;
pub mod room_service;
pub mod time_range;
pub mod validation;
