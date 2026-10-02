//! Business operations on rooms.

use std::sync::Arc;

use uuid::Uuid;

use crate::domain::{
    clock::Clock,
    error::{DomainError, DomainResult},
    repositories::{BookingRepository, RoomRepository},
    room::{NewRoom, Room, RoomFilter},
};

// A service is the entry point to business logic for one area of the domain.
// Handlers call services; services call repositories.
//
// Since step 012 the service depends on the `RoomRepository` *trait*, not on a concrete type.
// `Arc<dyn RoomRepository>` – a shared pointer to "some type implementing the trait";
// the concrete type is chosen at runtime by the composition root (`app.rs`).
// `domain` no longer imports anything from `infrastructure`.
pub struct RoomService {
    repository: Arc<dyn RoomRepository>,
    // Needed by the rule "a room with upcoming bookings can't be deleted" (step 013).
    bookings: Arc<dyn BookingRepository>,
    clock: Arc<dyn Clock>,
}

impl RoomService {
    // Constructor injection: dependencies are passed in, never created inside the service.
    pub fn new(
        repository: Arc<dyn RoomRepository>,
        bookings: Arc<dyn BookingRepository>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            repository,
            bookings,
            clock,
        }
    }

    // `?` converts `RepositoryError` into `DomainError::Repository` (`#[from]`).
    pub async fn list_rooms(&self, filter: &RoomFilter) -> DomainResult<Vec<Room>> {
        Ok(self.repository.find(filter).await?)
    }

    pub async fn get_room(&self, id: Uuid) -> DomainResult<Room> {
        // `ok_or_else` turns `Option<T>` into `Result<T, E>`; the closure builds the error
        // only when needed (`ok_or` would build it eagerly every time).
        self.repository
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::room_not_found(id))
    }

    pub async fn create_room(&self, new_room: NewRoom) -> DomainResult<Room> {
        Ok(self.repository.insert(new_room).await?)
    }

    pub async fn update_room(&self, id: Uuid, data: NewRoom) -> DomainResult<Room> {
        self.repository
            .update(id, data)
            .await?
            .ok_or_else(|| DomainError::room_not_found(id))
    }

    pub async fn delete_room(&self, id: Uuid) -> DomainResult<()> {
        // Business rule: deleting a room would silently invalidate bookings users rely on.
        let upcoming = self
            .bookings
            .count_active_by_room(id, self.clock.now())
            .await?;
        if upcoming > 0 {
            return Err(DomainError::Conflict(format!(
                "room {id} has {upcoming} upcoming booking(s); cancel them first"
            )));
        }
        if self.repository.delete(id).await? {
            Ok(())
        } else {
            Err(DomainError::room_not_found(id))
        }
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;
    use crate::{
        domain::{
            clock::SystemClock,
            repositories::{RepositoryError, RepositoryResult},
        },
        infrastructure::memory::InMemoryBookingRepository,
    };

    // Thanks to the trait, a test can inject any implementation – here a stub that always fails.
    // No database, no HTTP; tests the service's error handling in isolation.
    struct FailingRepository;

    fn failure() -> RepositoryError {
        RepositoryError {
            message: "connection lost".into(),
            source: None,
        }
    }

    #[async_trait]
    impl RoomRepository for FailingRepository {
        async fn find(&self, _: &RoomFilter) -> RepositoryResult<Vec<Room>> {
            Err(failure())
        }
        async fn find_by_id(&self, _: Uuid) -> RepositoryResult<Option<Room>> {
            Err(failure())
        }
        async fn insert(&self, _: NewRoom) -> RepositoryResult<Room> {
            Err(failure())
        }
        async fn update(&self, _: Uuid, _: NewRoom) -> RepositoryResult<Option<Room>> {
            Err(failure())
        }
        async fn delete(&self, _: Uuid) -> RepositoryResult<bool> {
            Err(failure())
        }
    }

    #[tokio::test]
    async fn repository_failure_becomes_domain_error() {
        let service = RoomService::new(
            Arc::new(FailingRepository),
            Arc::new(InMemoryBookingRepository::new()),
            Arc::new(SystemClock),
        );

        let result = service.get_room(Uuid::now_v7()).await;

        assert!(matches!(result, Err(DomainError::Repository(_))));
    }
}
