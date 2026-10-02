use std::{collections::HashMap, sync::RwLock};

use chrono::NaiveTime;
use uuid::Uuid;

use crate::domain::room::{NewRoom, Room, RoomFilter};

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
            let room = Room {
                id: Uuid::now_v7(),
                name: name.to_string(),
                description: None,
                capacity,
                opens_at: NaiveTime::from_hms_opt(opens, 0, 0).expect("valid sample hour"),
                closes_at: NaiveTime::from_hms_opt(closes, 0, 0).expect("valid sample hour"),
            };
            repository
                .rooms
                .write()
                .expect("rooms lock poisoned")
                .insert(room.id, room);
        }
        repository
    }

    // Methods are `async` although the in-memory version does no I/O: the signatures already
    // match a database-backed repository (step 015), so callers won't change.
    pub async fn find(&self, filter: &RoomFilter) -> Vec<Room> {
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
        result
    }

    pub async fn find_by_id(&self, id: Uuid) -> Option<Room> {
        // The guard returned by `read()` is dropped at the end of the statement,
        // releasing the lock; we return a clone, not a reference into the map.
        self.rooms
            .read()
            .expect("rooms lock poisoned")
            .get(&id)
            .cloned()
    }

    pub async fn insert(&self, new_room: NewRoom) -> Room {
        let room = Room {
            // UUID v7 = 48-bit Unix timestamp (ms) + random bits: unique without coordination
            // and roughly ordered by creation time (better B-tree index locality than random v4).
            id: Uuid::now_v7(),
            name: new_room.name,
            description: new_room.description,
            capacity: new_room.capacity,
            opens_at: new_room.opens_at,
            closes_at: new_room.closes_at,
        };
        self.rooms
            .write()
            .expect("rooms lock poisoned")
            .insert(room.id, room.clone());
        room
    }

    pub async fn update(&self, id: Uuid, data: NewRoom) -> Option<Room> {
        let mut rooms = self.rooms.write().expect("rooms lock poisoned");
        // `get_mut` returns `Option<&mut Room>`; `?` returns `None` when the id is unknown.
        let room = rooms.get_mut(&id)?;
        room.name = data.name;
        room.description = data.description;
        room.capacity = data.capacity;
        room.opens_at = data.opens_at;
        room.closes_at = data.closes_at;
        Some(room.clone())
    }

    pub async fn delete(&self, id: Uuid) -> bool {
        self.rooms
            .write()
            .expect("rooms lock poisoned")
            .remove(&id)
            .is_some()
    }
}

// Clippy (`new_without_default`): a type with `new()` taking no arguments should also implement
// `Default`, so it works with `..Default::default()`, `unwrap_or_default()` etc.
impl Default for InMemoryRoomRepository {
    fn default() -> Self {
        Self::new()
    }
}
