//! Infrastructure layer: technical implementations (storage, external systems).

pub mod memory;
pub mod postgres;
pub mod security;

// Alternative `RoomRepository` implementations, compiled only with their cargo feature (step 017).
// `#[cfg(feature = "...")]` removes the module (and its dependencies) from the build otherwise.
#[cfg(feature = "orm-diesel")]
pub mod diesel;
#[cfg(feature = "orm-sea")]
pub mod sea_orm;
