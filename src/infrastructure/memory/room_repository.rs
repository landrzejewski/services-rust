use std::{
    collections::HashMap,
    sync::{
        RwLock,
        atomic::{AtomicU64, Ordering},
    },
};

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
    rooms: RwLock<HashMap<u64, Room>>,
    // Id generator – plays the role of a database sequence. Atomics are lock-free;
    // `fetch_add` returns the previous value and increments in one indivisible step.
    // `Ordering::Relaxed` is enough: we only need unique numbers, not ordering with other memory.
    next_id: AtomicU64,
}

impl InMemoryRoomRepository {
    pub fn new() -> Self {
        Self {
            rooms: RwLock::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    /// Repository pre-filled with sample data.
    pub fn with_sample_data() -> Self {
        let repository = Self::new();
        for (name, capacity) in [("Blue room", 8), ("Green room", 4), ("Conference hall", 40)] {
            let room = Room {
                id: repository.generate_id(),
                name: name.to_string(),
                capacity,
            };
            repository
                .rooms
                .write()
                .expect("rooms lock poisoned")
                .insert(room.id, room);
        }
        repository
    }

    fn generate_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
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
        result.sort_by_key(|room| room.id);
        result
    }

    pub async fn find_by_id(&self, id: u64) -> Option<Room> {
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
            id: self.generate_id(),
            name: new_room.name,
            capacity: new_room.capacity,
        };
        self.rooms
            .write()
            .expect("rooms lock poisoned")
            .insert(room.id, room.clone());
        room
    }

    pub async fn update(&self, id: u64, data: NewRoom) -> Option<Room> {
        let mut rooms = self.rooms.write().expect("rooms lock poisoned");
        // `get_mut` returns `Option<&mut Room>`; `?` returns `None` when the id is unknown.
        let room = rooms.get_mut(&id)?;
        room.name = data.name;
        room.capacity = data.capacity;
        Some(room.clone())
    }

    pub async fn delete(&self, id: u64) -> bool {
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
