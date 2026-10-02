//! PostgreSQL implementation of the booking unit of work (step 016).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::{booking_repository, room_repository};
use crate::domain::{
    booking::{Booking, NewBooking},
    repositories::{BookingTransaction, BookingUnitOfWork, RepositoryResult},
    room::Room,
    time_range::TimeRange,
};

pub struct PostgresBookingUnitOfWork {
    pool: PgPool,
}

impl PostgresBookingUnitOfWork {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl BookingUnitOfWork for PostgresBookingUnitOfWork {
    async fn begin(&self) -> RepositoryResult<Box<dyn BookingTransaction>> {
        // `pool.begin()` takes a connection from the pool and sends `BEGIN`.
        // Default isolation level in PostgreSQL: READ COMMITTED. Locks (below) make it safe here;
        // alternatively run `SET TRANSACTION ISOLATION LEVEL SERIALIZABLE` as the first statement
        // and retry on serialization failures (SQLSTATE 40001).
        let tx = self.pool.begin().await?;
        Ok(Box::new(PostgresBookingTransaction { tx }))
    }
}

/// Wraps `sqlx::Transaction`. It holds one connection exclusively until commit/rollback.
/// `'static` – the transaction owns its pooled connection (not borrowed from a local).
struct PostgresBookingTransaction {
    tx: Transaction<'static, Postgres>,
}

#[async_trait]
impl BookingTransaction for PostgresBookingTransaction {
    async fn lock_room(&mut self, room_id: Uuid) -> RepositoryResult<Option<Room>> {
        // `&mut *self.tx` – reborrow the transaction as `&mut PgConnection` (an executor).
        // Every statement below runs on the same connection, inside the same transaction.
        room_repository::select_room_for_update(&mut *self.tx, room_id).await
    }

    async fn lock_user(&mut self, user_id: Uuid) -> RepositoryResult<()> {
        // Advisory lock – an application-defined lock on a number, released automatically at
        // the end of the transaction (`_xact_`). There is no "user" row to lock with FOR UPDATE
        // (users arrive in step 018), so the user id is hashed into a 64-bit lock key.
        sqlx::query!(
            "SELECT pg_advisory_xact_lock(hashtextextended($1::text, 0))",
            user_id.to_string()
        )
        .execute(&mut *self.tx)
        .await?;
        Ok(())
    }

    async fn find_active_overlapping(
        &mut self,
        room_id: Uuid,
        period: &TimeRange,
    ) -> RepositoryResult<Vec<Booking>> {
        booking_repository::select_active_overlapping(&mut *self.tx, room_id, period).await
    }

    async fn count_active_by_user(
        &mut self,
        user_id: Uuid,
        from: DateTime<Utc>,
    ) -> RepositoryResult<usize> {
        booking_repository::count_active_by_user(&mut *self.tx, user_id, from).await
    }

    async fn insert_booking(&mut self, new_booking: NewBooking) -> RepositoryResult<Booking> {
        booking_repository::insert_booking(&mut *self.tx, new_booking).await
    }

    async fn commit(self: Box<Self>) -> RepositoryResult<()> {
        // COMMIT releases all locks. If `commit` is never called, dropping `Transaction`
        // issues a ROLLBACK (asynchronously, when the connection returns to the pool).
        self.tx.commit().await?;
        Ok(())
    }
}
