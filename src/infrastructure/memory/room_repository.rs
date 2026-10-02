use std::{collections::HashMap, sync::RwLock};

use crate::domain::room::Room;

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
}

impl InMemoryRoomRepository {
    pub fn new() -> Self {
        Self {
            rooms: RwLock::new(HashMap::new()),
        }
    }

    /// Repository pre-filled with sample data.
    pub fn with_sample_data() -> Self {
        let rooms = [
            (1, "Blue room", 8),
            (2, "Green room", 4),
            (3, "Conference hall", 40),
        ]
        .into_iter()
        .map(|(id, name, capacity)| {
            (
                id,
                Room {
                    id,
                    name: name.to_string(),
                    capacity,
                },
            )
        })
        .collect();

        Self {
            rooms: RwLock::new(rooms),
        }
    }

    // Methods are `async` although the in-memory version does no I/O: the signatures already
    // match a database-backed repository (step 015), so callers won't change.
    pub async fn find_all(&self) -> Vec<Room> {
        // `read()` returns `Err` only if another thread panicked while holding the lock
        // ("poisoned" lock). Data may then be inconsistent, so panicking is a reasonable choice.
        let rooms = self.rooms.read().expect("rooms lock poisoned");
        let mut result: Vec<Room> = rooms.values().cloned().collect();
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
}

// Clippy (`new_without_default`): a type with `new()` taking no arguments should also implement
// `Default`, so it works with `..Default::default()`, `unwrap_or_default()` etc.
impl Default for InMemoryRoomRepository {
    fn default() -> Self {
        Self::new()
    }
}
