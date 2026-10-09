use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use chrono::NaiveTime;
use uuid::Uuid;

use async_trait::async_trait;

use super::transaction::in_memory;
use crate::domain::{
    pagination::{Page, PageRequest},
    repositories::{RepositoryResult, RoomRepository},
    room::{NewRoom, OpeningHours, Room, RoomFilter, RoomName},
    transaction::Transaction,
};

pub struct InMemoryRoomRepository {
    rooms: Arc<RwLock<HashMap<Uuid, Room>>>,
}

impl InMemoryRoomRepository {
    pub fn new() -> Self {
        Self {
            rooms: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Repository pre-filled with sample data.
    pub fn with_sample_data() -> Self {
        let repository = Self::new();
        let samples = [
            ("Blue room", 8, (8, 18)),
            ("Green room", 4, (8, 18)),
            ("Conference hall", 40, (7, 22)),
        ];
        for (name, capacity, (opens, closes)) in samples {
            let hour = |h| NaiveTime::from_hms_opt(h, 0, 0).expect("valid sample hour");
            let room = Room {
                id: Uuid::now_v7(),
                name: RoomName::parse(name).expect("valid sample name"),
                description: None,
                capacity,
                opening_hours: OpeningHours::new(hour(opens), hour(closes))
                    .expect("valid sample hours"),
            };
            repository
                .rooms
                .write()
                .expect("rooms lock poisoned")
                .insert(room.id, room);
        }
        repository
    }
}

// The trait implementation – the *adapter* plugging this storage into the domain port.
// The in-memory version never fails, so every method returns `Ok(...)`.
#[async_trait]
impl RoomRepository for InMemoryRoomRepository {
    async fn find(&self, filter: &RoomFilter, page: PageRequest) -> RepositoryResult<Page<Room>> {
        // `read()` returns `Err` only if another thread panicked while holding the lock
        // ("poisoned" lock). Data may then be inconsistent, so panicking is a reasonable choice.
        let rooms = self.rooms.read().expect("rooms lock poisoned");
        let mut result: Vec<Room> = rooms
            .values()
            .filter(|room| filter.matches(room))
            .cloned()
            .collect();
        // HashMap has no order – sort to get a stable API response.
        // UUID v7 starts with a timestamp, so sorting by id = sorting by creation time.
        result.sort_by_key(|room| room.id);
        Ok(paginate(result, page))
    }

    async fn find_by_id(&self, id: Uuid) -> RepositoryResult<Option<Room>> {
        // The guard returned by `read()` is dropped at the end of the statement,
        // releasing the lock; we return a clone, not a reference into the map.
        Ok(self
            .rooms
            .read()
            .expect("rooms lock poisoned")
            .get(&id)
            .cloned())
    }

    async fn insert(&self, new_room: NewRoom) -> RepositoryResult<Room> {
        let room = Room {
            // UUID v7 = 48-bit Unix timestamp (ms) + random bits: unique without coordination
            // and roughly ordered by creation time (better B-tree index locality than random v4).
            id: Uuid::now_v7(),
            name: new_room.name,
            description: new_room.description,
            capacity: new_room.capacity,
            opening_hours: new_room.opening_hours,
        };
        self.rooms
            .write()
            .expect("rooms lock poisoned")
            .insert(room.id, room.clone());
        Ok(room)
    }

    async fn update(&self, id: Uuid, data: NewRoom) -> RepositoryResult<Option<Room>> {
        let mut rooms = self.rooms.write().expect("rooms lock poisoned");
        // `get_mut` returns `Option<&mut Room>`.
        let Some(room) = rooms.get_mut(&id) else {
            return Ok(None);
        };
        room.name = data.name;
        room.description = data.description;
        room.capacity = data.capacity;
        room.opening_hours = data.opening_hours;
        Ok(Some(room.clone()))
    }

    async fn find_by_id_for_update(
        &self,
        tx: &mut dyn Transaction,
        id: Uuid,
    ) -> RepositoryResult<Option<Room>> {
        // The global lock of `InMemoryTxManager` already serializes transactions – a plain read
        // is enough. `in_memory(tx)?` only checks the transaction belongs to this storage.
        in_memory(tx)?;
        self.find_by_id(id).await
    }

    async fn delete(&self, tx: &mut dyn Transaction, id: Uuid) -> RepositoryResult<bool> {
        let tx = in_memory(tx)?;
        let exists = self
            .rooms
            .read()
            .expect("rooms lock poisoned")
            .contains_key(&id);
        let rooms = Arc::clone(&self.rooms);
        tx.defer(move || {
            rooms.write().expect("rooms lock poisoned").remove(&id);
        });
        Ok(exists)
    }
}

/// Cuts one page out of a fully loaded, sorted list.
pub(super) fn paginate<T>(items: Vec<T>, page: PageRequest) -> Page<T> {
    let total = items.len() as u64;
    let items = items
        .into_iter()
        .skip(usize::try_from(page.offset()).unwrap_or(usize::MAX))
        .take(page.size() as usize)
        .collect();
    Page {
        items,
        request: page,
        total,
    }
}

// Clippy (`new_without_default`): a type with `new()` taking no arguments should also implement
// `Default`, so it works with `..Default::default()`, `unwrap_or_default()` etc.
impl Default for InMemoryRoomRepository {
    fn default() -> Self {
        Self::new()
    }
}
