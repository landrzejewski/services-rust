use std::any::Any;

use async_trait::async_trait;

use crate::domain::repositories::RepositoryResult;

#[async_trait]
pub trait Transaction: Send {
    async fn commit(self: Box<Self>) -> RepositoryResult<()>;

    async fn rollback(self: Box<Self>) -> RepositoryResult<()>;

    fn as_any_mut(&mut self) -> &mut dyn Any;
}

#[async_trait]
pub trait TxManager: Send + Sync {
    async fn begin(&self) -> RepositoryResult<Box<dyn Transaction>>;
}
