//! Business operations on rooms.

use std::sync::Arc;

use uuid::Uuid;

use crate::{
    domain::{
        error::{DomainError, DomainResult},
        room::{NewRoom, Room, RoomFilter},
    },
    infrastructure::memory::InMemoryRoomRepository,
};

// A service is the entry point to business logic for one area of the domain.
// Handlers call services; services call repositories.
//
// The service currently depends on a *concrete* repository type. This works, but couples
// the domain to infrastructure (`domain` imports `infrastructure`) and makes it hard to swap
// the storage or mock it in tests. Step 012 inverts this dependency with a trait.
pub struct RoomService {
    // `Arc` – the repository is shared; `BookingService` uses the same instance.
    repository: Arc<InMemoryRoomRepository>,
}

impl RoomService {
    pub fn new(repository: Arc<InMemoryRoomRepository>) -> Self {
        Self { repository }
    }

    // Since step 010 every operation returns `DomainResult<T>`, even those that can't fail yet:
    // with a database (step 015) every call can fail, and callers already handle it.
    pub async fn list_rooms(&self, filter: &RoomFilter) -> DomainResult<Vec<Room>> {
        Ok(self.repository.find(filter).await)
    }

    pub async fn get_room(&self, id: Uuid) -> DomainResult<Room> {
        // `ok_or_else` turns `Option<T>` into `Result<T, E>`; the closure builds the error
        // only when needed (`ok_or` would build it eagerly every time).
        self.repository
            .find_by_id(id)
            .await
            .ok_or_else(|| DomainError::room_not_found(id))
    }

    pub async fn create_room(&self, new_room: NewRoom) -> DomainResult<Room> {
        Ok(self.repository.insert(new_room).await)
    }

    pub async fn update_room(&self, id: Uuid, data: NewRoom) -> DomainResult<Room> {
        self.repository
            .update(id, data)
            .await
            .ok_or_else(|| DomainError::room_not_found(id))
    }

    pub async fn delete_room(&self, id: Uuid) -> DomainResult<()> {
        if self.repository.delete(id).await {
            Ok(())
        } else {
            Err(DomainError::room_not_found(id))
        }
    }
}
