use std::{any::Any, sync::Arc};

use async_trait::async_trait;
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::domain::{
    repositories::{RepositoryError, RepositoryResult},
    transaction::{Transaction, TxManager},
};

pub struct InMemoryTxManager {
    lock: Arc<Mutex<()>>,
}

impl InMemoryTxManager {
    pub fn new() -> Self {
        Self {
            lock: Arc::new(Mutex::new(())),
        }
    }
}

impl Default for InMemoryTxManager {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TxManager for InMemoryTxManager {
    async fn begin(&self) -> RepositoryResult<Box<dyn Transaction>> {
        // `tokio::sync::Mutex` (not `std`): the guard lives across `.await` points for the whole
        // transaction. `lock_owned` returns a guard that owns an `Arc` to the mutex, so it can be
        // stored in a struct with no borrowed lifetime.
        let guard = Arc::clone(&self.lock).lock_owned().await;
        Ok(Box::new(InMemoryTransaction {
            _guard: guard,
            pending: Vec::new(),
        }))
    }
}

/// A deferred write – applied on commit, discarded on rollback.
type PendingWrite = Box<dyn FnOnce() + Send>;

/// Writes are buffered and applied on commit, so a dropped transaction leaves no trace.
/// Reads inside the transaction see committed data only (no "read your own writes") – enough for
/// the services, which never read after writing in the same transaction.
pub(super) struct InMemoryTransaction {
    // Held until the transaction is committed or dropped; never read (hence `_`).
    _guard: OwnedMutexGuard<()>,
    pending: Vec<PendingWrite>,
}

impl InMemoryTransaction {
    /// Called by the repositories for every write made inside the transaction.
    pub(super) fn defer(&mut self, write: impl FnOnce() + Send + 'static) {
        self.pending.push(Box::new(write));
    }
}

#[async_trait]
impl Transaction for InMemoryTransaction {
    async fn commit(self: Box<Self>) -> RepositoryResult<()> {
        for write in self.pending {
            write();
        }
        Ok(())
        // `_guard` is dropped here -> the next transaction may start.
    }

    async fn rollback(self: Box<Self>) -> RepositoryResult<()> {
        // Nothing to undo – pending writes are simply dropped together with `self`.
        Ok(())
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Counterpart of `postgres::transaction::connection` – the in-memory transaction behind
/// `&mut dyn Transaction`, or an error if the service passed a transaction of another storage.
pub(super) fn in_memory(tx: &mut dyn Transaction) -> RepositoryResult<&mut InMemoryTransaction> {
    tx.as_any_mut()
        .downcast_mut::<InMemoryTransaction>()
        .ok_or_else(|| RepositoryError::Unexpected {
            message: "transaction does not belong to the in-memory storage".into(),
            source: None,
        })
}

#[cfg(test)]
mod tests {
    use chrono::NaiveTime;

    use super::*;
    use crate::{
        domain::{
            repositories::RoomRepository,
            room::{NewRoom, OpeningHours, RoomName},
        },
        infrastructure::memory::InMemoryRoomRepository,
    };

    async fn room_repository_with_one_room() -> (InMemoryRoomRepository, uuid::Uuid) {
        let rooms = InMemoryRoomRepository::new();
        let hour = |h| NaiveTime::from_hms_opt(h, 0, 0).unwrap();
        let room = rooms
            .insert(NewRoom {
                name: RoomName::parse("Test room").unwrap(),
                description: None,
                capacity: 4,
                opening_hours: OpeningHours::new(hour(8), hour(18)).unwrap(),
            })
            .await
            .unwrap();
        (rooms, room.id)
    }

    #[tokio::test]
    async fn writes_are_applied_on_commit_only() {
        let (rooms, id) = room_repository_with_one_room().await;
        let tx_manager = InMemoryTxManager::new();

        let mut tx = tx_manager.begin().await.unwrap();
        assert!(rooms.delete(tx.as_mut(), id).await.unwrap());
        assert!(rooms.find_by_id(id).await.unwrap().is_some());
        tx.commit().await.unwrap();

        assert!(rooms.find_by_id(id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn rolled_back_and_dropped_transactions_leave_no_trace() {
        let (rooms, id) = room_repository_with_one_room().await;
        let tx_manager = InMemoryTxManager::new();

        let mut tx = tx_manager.begin().await.unwrap();
        rooms.delete(tx.as_mut(), id).await.unwrap();
        tx.rollback().await.unwrap();

        let mut tx = tx_manager.begin().await.unwrap();
        rooms.delete(tx.as_mut(), id).await.unwrap();
        drop(tx);

        assert!(rooms.find_by_id(id).await.unwrap().is_some());
    }

    // The price of `dyn`: nothing stops a caller from passing a transaction of another storage.
    // The adapter detects it at runtime.
    struct ForeignTransaction;

    #[async_trait]
    impl Transaction for ForeignTransaction {
        async fn commit(self: Box<Self>) -> RepositoryResult<()> {
            Ok(())
        }
        async fn rollback(self: Box<Self>) -> RepositoryResult<()> {
            Ok(())
        }
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[tokio::test]
    async fn foreign_transaction_is_rejected() {
        let (rooms, id) = room_repository_with_one_room().await;

        let result = rooms.delete(&mut ForeignTransaction, id).await;

        assert!(matches!(result, Err(RepositoryError::Unexpected { .. })));
    }
}
