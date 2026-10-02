//! Business operations on rooms.

use std::sync::Arc;

use crate::{domain::room::Room, infrastructure::memory::InMemoryRoomRepository};

// A service is the entry point to business logic for one area of the domain.
// Handlers call services; services call repositories.
//
// The service currently depends on a *concrete* repository type. This works, but couples
// the domain to infrastructure (`domain` imports `infrastructure`) and makes it hard to swap
// the storage or mock it in tests. Step 012 inverts this dependency with a trait.
pub struct RoomService {
    // `Arc` – the repository is shared; several services may use the same instance later.
    repository: Arc<InMemoryRoomRepository>,
}

impl RoomService {
    pub fn new(repository: Arc<InMemoryRoomRepository>) -> Self {
        Self { repository }
    }

    pub async fn list_rooms(&self) -> Vec<Room> {
        self.repository.find_all().await
    }

    /// `None` = room does not exist. Error types replace `Option` in step 010.
    pub async fn get_room(&self, id: u64) -> Option<Room> {
        self.repository.find_by_id(id).await
    }
}
