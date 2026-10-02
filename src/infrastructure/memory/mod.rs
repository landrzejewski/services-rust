//! In-memory storage – no database needed, data is lost on restart.
//! Useful for the first steps of the course and later for fast tests.

mod booking_repository;
mod room_repository;
mod unit_of_work;
mod user_repository;

// Re-export: callers write `infrastructure::memory::InMemoryRoomRepository`
// instead of the longer `infrastructure::memory::room_repository::InMemoryRoomRepository`.
pub use booking_repository::InMemoryBookingRepository;
pub use room_repository::InMemoryRoomRepository;
pub use unit_of_work::InMemoryBookingUnitOfWork;
pub use user_repository::InMemoryUserRepository;
