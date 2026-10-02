//! Business operations on rooms.

use std::sync::Arc;

use uuid::Uuid;

use crate::{
    domain::room::{NewRoom, Room, RoomFilter},
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

    pub async fn list_rooms(&self, filter: &RoomFilter) -> Vec<Room> {
        self.repository.find(filter).await
    }

    /// `None` = room does not exist. Error types replace `Option` in step 010.
    pub async fn get_room(&self, id: Uuid) -> Option<Room> {
        self.repository.find_by_id(id).await
    }

    pub async fn create_room(&self, new_room: NewRoom) -> Room {
        self.repository.insert(new_room).await
    }

    pub async fn update_room(&self, id: Uuid, data: NewRoom) -> Option<Room> {
        self.repository.update(id, data).await
    }

    /// Returns `false` when the room did not exist.
    pub async fn delete_room(&self, id: Uuid) -> bool {
        self.repository.delete(id).await
    }
}
