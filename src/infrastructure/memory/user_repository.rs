use std::{collections::HashMap, sync::RwLock};

use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

use crate::domain::{
    repositories::{RepositoryError, RepositoryResult, UserRepository},
    user::{Email, NewUser, Role, User},
};

/// In-memory users – for tests.
#[derive(Default)]
pub struct InMemoryUserRepository {
    users: RwLock<HashMap<Uuid, User>>,
}

#[async_trait]
impl UserRepository for InMemoryUserRepository {
    async fn find_by_id(&self, id: Uuid) -> RepositoryResult<Option<User>> {
        Ok(self
            .users
            .read()
            .expect("users lock poisoned")
            .get(&id)
            .cloned())
    }

    async fn find_by_email(&self, email: &Email) -> RepositoryResult<Option<User>> {
        Ok(self
            .users
            .read()
            .expect("users lock poisoned")
            .values()
            .find(|user| &user.email == email)
            .cloned())
    }

    async fn insert(&self, new_user: NewUser) -> RepositoryResult<User> {
        let mut users = self.users.write().expect("users lock poisoned");
        if users.values().any(|user| user.email == new_user.email) {
            return Err(RepositoryError::Conflict(
                "a user with this e-mail already exists".to_string(),
            ));
        }
        let user = User {
            id: Uuid::now_v7(),
            email: new_user.email,
            password_hash: new_user.password_hash,
            role: new_user.role,
            created_at: Utc::now(),
        };
        users.insert(user.id, user.clone());
        Ok(user)
    }

    async fn upsert_external(&self, id: Uuid, email: &Email, role: Role) -> RepositoryResult<()> {
        let mut users = self.users.write().expect("users lock poisoned");
        let user = users.entry(id).or_insert_with(|| User {
            id,
            email: email.clone(),
            password_hash: None,
            role,
            created_at: Utc::now(),
        });
        user.email = email.clone();
        user.role = role;
        Ok(())
    }
}
