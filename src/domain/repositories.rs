//! Repository traits – the domain's *ports* to storage.
//!
//! Dependency inversion (step 012): the domain declares WHAT it needs from storage;
//! infrastructure provides HOW (in-memory now, PostgreSQL from step 015). Dependencies now point
//! from infrastructure to domain, never the other way round:
//!
//! ```text
//!   domain::RoomService ──uses──▶ domain::RoomRepository (trait)
//!                                        ▲ implements
//!   infrastructure::InMemoryRoomRepository / PostgresRoomRepository
//! ```

use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::{
    booking::{Booking, BookingFilter, BookingStatus, NewBooking},
    room::{NewRoom, Room, RoomFilter},
};

/// Storage failure (connection lost, timeout, constraint the domain didn't expect...).
///
/// The trait must express that any implementation can fail, even if the in-memory one never
/// does. The source error is kept (boxed) for logging, but the domain doesn't depend on
/// driver-specific error types like `sqlx::Error`.
#[derive(Debug, thiserror::Error)]
#[error("repository error: {message}")]
pub struct RepositoryError {
    pub message: String,
    // `#[source]` – exposed via `Error::source()`, so loggers can print the whole chain.
    #[source]
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

pub type RepositoryResult<T> = Result<T, RepositoryError>;

// Why `#[async_trait]`: `async fn` in traits is stable, but such traits are not
// *dyn-compatible* – `Arc<dyn RoomRepository>` would not compile. The macro rewrites
// `async fn f(&self) -> T` into `fn f(&self) -> Pin<Box<dyn Future<Output = T> + Send + '_>>`,
// which works with trait objects at the cost of one heap allocation per call
// (negligible next to I/O).
//
// `Send + Sync` supertraits: the repository is shared between threads (`Arc`) and used inside
// spawned tasks (handlers), so every implementation must be thread-safe.
#[async_trait]
pub trait RoomRepository: Send + Sync {
    async fn find(&self, filter: &RoomFilter) -> RepositoryResult<Vec<Room>>;
    async fn find_by_id(&self, id: Uuid) -> RepositoryResult<Option<Room>>;
    async fn insert(&self, new_room: NewRoom) -> RepositoryResult<Room>;
    /// `None` when the room does not exist.
    async fn update(&self, id: Uuid, data: NewRoom) -> RepositoryResult<Option<Room>>;
    /// `false` when the room did not exist.
    async fn delete(&self, id: Uuid) -> RepositoryResult<bool>;
}

#[async_trait]
pub trait BookingRepository: Send + Sync {
    async fn find(&self, filter: &BookingFilter) -> RepositoryResult<Vec<Booking>>;
    async fn find_by_id(&self, id: Uuid) -> RepositoryResult<Option<Booking>>;
    async fn insert(&self, new_booking: NewBooking) -> RepositoryResult<Booking>;
    async fn update_status(
        &self,
        id: Uuid,
        status: BookingStatus,
    ) -> RepositoryResult<Option<Booking>>;
}
