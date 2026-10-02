use std::{collections::HashMap, sync::RwLock};

use chrono::NaiveTime;
use uuid::Uuid;

use async_trait::async_trait;

use crate::domain::{
    repositories::{RepositoryResult, RoomRepository},
    room::{NewRoom, OpeningHours, Room, RoomFilter, RoomName},
};

// Repository – hides *how* data is stored; offers collection-like operations to the domain.
//
// Handlers run concurrently on many threads, so shared mutable data needs synchronization.
// `RwLock` allows many readers or one writer at a time.
//
// `std::sync::RwLock` (not `tokio::sync::RwLock`) is the right choice here: the lock is held only
// for a few nanoseconds and NEVER across an `.await`. The async lock is needed only when a guard
// must live across `.await` points; it is slower otherwise.
pub struct InMemoryRoomRepository {
    rooms: RwLock<HashMap<Uuid, Room>>,
}

impl InMemoryRoomRepository {
    pub fn new() -> Self {
        Self {
            rooms: RwLock::new(HashMap::new()),
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
    async fn find(&self, filter: &RoomFilter) -> RepositoryResult<Vec<Room>> {
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
        Ok(result)
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

    async fn delete(&self, id: Uuid) -> RepositoryResult<bool> {
        Ok(self
            .rooms
            .write()
            .expect("rooms lock poisoned")
            .remove(&id)
            .is_some())
    }
}

// Clippy (`new_without_default`): a type with `new()` taking no arguments should also implement
// `Default`, so it works with `..Default::default()`, `unwrap_or_default()` etc.
impl Default for InMemoryRoomRepository {
    fn default() -> Self {
        Self::new()
    }
}
