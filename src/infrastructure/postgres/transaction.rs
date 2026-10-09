//! PostgreSQL implementation of `TxManager` / `Transaction` (step 016).

use std::any::Any;

use async_trait::async_trait;
use sqlx::{PgConnection, PgPool, Postgres};

use crate::domain::{
    repositories::{RepositoryError, RepositoryResult},
    transaction::{Transaction, TxManager},
};

pub struct PostgresTxManager {
    pool: PgPool,
}

impl PostgresTxManager {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TxManager for PostgresTxManager {
    async fn begin(&self) -> RepositoryResult<Box<dyn Transaction>> {
        // `pool.begin()` takes a connection from the pool and sends `BEGIN`.
        // Default isolation level in PostgreSQL: READ COMMITTED. Explicit locks (`FOR UPDATE`,
        // advisory locks in the repositories) make it safe here; alternatively run
        // `SET TRANSACTION ISOLATION LEVEL SERIALIZABLE` as the first statement and retry on
        // serialization failures (SQLSTATE 40001).
        let tx = self.pool.begin().await?;
        Ok(Box::new(PostgresTransaction { tx }))
    }
}

/// Wraps `sqlx::Transaction`. It holds one connection exclusively until commit/rollback.
/// `'static` – the transaction owns its pooled connection (not borrowed from a local),
/// which is also what `Any` requires.
struct PostgresTransaction {
    tx: sqlx::Transaction<'static, Postgres>,
}

#[async_trait]
impl Transaction for PostgresTransaction {
    async fn commit(self: Box<Self>) -> RepositoryResult<()> {
        // COMMIT releases all locks. If `commit` is never called, dropping `sqlx::Transaction`
        // issues a ROLLBACK (asynchronously, when the connection returns to the pool).
        self.tx.commit().await?;
        Ok(())
    }

    async fn rollback(self: Box<Self>) -> RepositoryResult<()> {
        self.tx.rollback().await?;
        Ok(())
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The connection of the transaction the service passed in – what the repositories run their
/// queries on, so every statement takes part in the same transaction.
///
/// `downcast_mut` succeeds only if `tx` really is a `PostgresTransaction`. Anything else
/// (e.g. an in-memory transaction wired by mistake) is a programming error the compiler can't
/// catch with `dyn` – reported as an unexpected repository error.
pub(super) fn connection(tx: &mut dyn Transaction) -> RepositoryResult<&mut PgConnection> {
    tx.as_any_mut()
        .downcast_mut::<PostgresTransaction>()
        // `&mut *t.tx` – `sqlx::Transaction` derefs to `PgConnection` (an executor).
        .map(|t| &mut *t.tx)
        .ok_or_else(|| RepositoryError::Unexpected {
            message: "transaction does not belong to the PostgreSQL storage".into(),
            source: None,
        })
}
