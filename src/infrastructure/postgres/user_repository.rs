use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{
    repositories::{RepositoryError, RepositoryResult, UserRepository},
    user::{Email, NewUser, Role, User},
};

/// `UserRepository` backed by PostgreSQL (step 018).
pub struct PostgresUserRepository {
    pool: PgPool,
}

impl PostgresUserRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

struct UserRow {
    id: Uuid,
    email: String,
    password_hash: Option<String>,
    role: String,
    created_at: DateTime<Utc>,
}

pub(super) fn role_to_db(role: Role) -> &'static str {
    match role {
        Role::User => "USER",
        Role::Admin => "ADMIN",
    }
}

fn role_from_db(value: &str) -> RepositoryResult<Role> {
    match value {
        "USER" => Ok(Role::User),
        "ADMIN" => Ok(Role::Admin),
        other => Err(RepositoryError::Unexpected {
            message: format!("unknown role in database: {other}"),
            source: None,
        }),
    }
}

impl TryFrom<UserRow> for User {
    type Error = RepositoryError;

    fn try_from(row: UserRow) -> Result<Self, Self::Error> {
        Ok(User {
            id: row.id,
            email: Email::parse(&row.email)
                .map_err(|e| RepositoryError::unexpected("invalid user row", e))?,
            password_hash: row.password_hash,
            role: role_from_db(&row.role)?,
            created_at: row.created_at,
        })
    }
}

#[async_trait]
impl UserRepository for PostgresUserRepository {
    async fn find_by_id(&self, id: Uuid) -> RepositoryResult<Option<User>> {
        sqlx::query_as!(
            UserRow,
            "SELECT id, email, password_hash, role, created_at FROM users WHERE id = $1",
            id
        )
        .fetch_optional(&self.pool)
        .await?
        .map(User::try_from)
        .transpose()
    }

    async fn find_by_email(&self, email: &Email) -> RepositoryResult<Option<User>> {
        // `lower(email)` matches the expression of the unique index `users_email_idx`,
        // so the lookup uses the index.
        sqlx::query_as!(
            UserRow,
            r#"
            SELECT id, email, password_hash, role, created_at
            FROM users WHERE lower(email) = lower($1)
            "#,
            email.as_str()
        )
        .fetch_optional(&self.pool)
        .await?
        .map(User::try_from)
        .transpose()
    }

    async fn insert(&self, new_user: NewUser) -> RepositoryResult<User> {
        sqlx::query_as!(
            UserRow,
            r#"
            INSERT INTO users (id, email, password_hash, role)
            VALUES ($1, $2, $3, $4)
            RETURNING id, email, password_hash, role, created_at
            "#,
            Uuid::now_v7(),
            new_user.email.as_str(),
            new_user.password_hash,
            role_to_db(new_user.role),
        )
        .fetch_one(&self.pool)
        .await?
        .try_into()
    }
}
