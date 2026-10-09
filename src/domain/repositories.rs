use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::domain::{
    booking::{Booking, BookingFilter, BookingStatus, NewBooking},
    pagination::{Page, PageRequest},
    room::{NewRoom, Room, RoomFilter},
    time_range::TimeRange,
    transaction::Transaction,
};

#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    /// The storage rejected the change because it conflicts with existing data
    /// (unique / exclusion constraint). Meaningful to the domain -> `DomainError::Conflict`.
    #[error("{0}")]
    Conflict(String),

    /// Anything else: connection lost, timeout, corrupted data... -> HTTP 500.
    #[error("repository error: {message}")]
    Unexpected {
        message: String,
        // `#[source]` – exposed via `Error::source()`, so loggers can print the whole chain.
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
}

impl RepositoryError {
    pub fn unexpected(
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Unexpected {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }
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
    async fn find(&self, filter: &RoomFilter, page: PageRequest) -> RepositoryResult<Page<Room>>;
    async fn find_by_id(&self, id: Uuid) -> RepositoryResult<Option<Room>>;
    async fn insert(&self, new_room: NewRoom) -> RepositoryResult<Room>;
    /// `None` when the room does not exist.
    async fn update(&self, id: Uuid, data: NewRoom) -> RepositoryResult<Option<Room>>;

    // Operations taking `&mut dyn Transaction` (step 016) run inside the caller's transaction.
    // The service starts it with `TxManager::begin` and passes it down; the repository never
    // decides where a transaction begins or ends.

    /// Loads the room and locks it until the end of the transaction (`SELECT ... FOR UPDATE`).
    /// Concurrent transactions locking the same room wait – bookings of one room and its
    /// deletion are serialized.
    async fn find_by_id_for_update(
        &self,
        tx: &mut dyn Transaction,
        id: Uuid,
    ) -> RepositoryResult<Option<Room>>;

    /// `false` when the room did not exist.
    async fn delete(&self, tx: &mut dyn Transaction, id: Uuid) -> RepositoryResult<bool>;
}

#[async_trait]
pub trait BookingRepository: Send + Sync {
    async fn find(
        &self,
        filter: &BookingFilter,
        page: PageRequest,
    ) -> RepositoryResult<Page<Booking>>;
    async fn find_by_id(&self, id: Uuid) -> RepositoryResult<Option<Booking>>;
    async fn update_status(
        &self,
        id: Uuid,
        status: BookingStatus,
    ) -> RepositoryResult<Option<Booking>>;

    // Transactional operations of the "create booking" / "delete room" use cases (step 016).
    // Queries needed by business rules (step 013) are dedicated methods instead of loading all
    // bookings and filtering in Rust: a database answers them with one indexed query.

    async fn insert(
        &self,
        tx: &mut dyn Transaction,
        new_booking: NewBooking,
    ) -> RepositoryResult<Booking>;

    /// Serializes concurrent bookings of the same user (for the per-user limit), even when they
    /// target different rooms. Released at the end of the transaction.
    async fn lock_user(&self, tx: &mut dyn Transaction, user_id: Uuid) -> RepositoryResult<()>;

    /// Active bookings of the room whose period overlaps `period`.
    async fn find_active_overlapping(
        &self,
        tx: &mut dyn Transaction,
        room_id: Uuid,
        period: &TimeRange,
    ) -> RepositoryResult<Vec<Booking>>;

    /// Number of active bookings of the user that end after `from` (upcoming or ongoing).
    async fn count_active_by_user(
        &self,
        tx: &mut dyn Transaction,
        user_id: Uuid,
        from: DateTime<Utc>,
    ) -> RepositoryResult<usize>;

    /// Number of active bookings of the room that end after `from`.
    async fn count_active_by_room(
        &self,
        tx: &mut dyn Transaction,
        room_id: Uuid,
        from: DateTime<Utc>,
    ) -> RepositoryResult<usize>;
}
