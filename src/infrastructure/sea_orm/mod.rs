//! `RoomRepository` implemented with SeaORM (step 017, cargo feature `orm-sea`).
//!
//! SeaORM: async, dynamic ORM built on sqlx + SeaQuery (query builder).
//! - entities are Rust structs (`Model`) with derive macros, queries are built with methods
//!   (no SQL strings), checked at runtime (not at compile time like `sqlx::query!`),
//! - `ActiveModel` tracks which fields are set/changed for INSERT/UPDATE,
//! - relations, pagination, transactions, migrations (`sea-orm-migration`), code generation
//!   from an existing schema (`sea-orm-cli generate entity`).

mod room_entity;

use async_trait::async_trait;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ActiveValue::Unchanged, ColumnTrait, DatabaseConnection,
    DbErr, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, SqlErr, SqlxPostgresConnector,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    domain::{
        pagination::{Page, PageRequest},
        repositories::{RepositoryError, RepositoryResult, RoomRepository},
        room::{NewRoom, Room, RoomFilter},
    },
    infrastructure::postgres::{like_pattern, room_from_columns, to_db_int},
};

pub struct SeaOrmRoomRepository {
    db: DatabaseConnection,
}

impl SeaOrmRoomRepository {
    /// SeaORM 2 uses sqlx 0.9 internally, so it can reuse the application's existing pool –
    /// no second set of connections.
    pub fn new(pool: PgPool) -> Self {
        Self {
            db: SqlxPostgresConnector::from_sqlx_postgres_pool(pool),
        }
    }
}

fn to_domain(model: room_entity::Model) -> RepositoryResult<Room> {
    room_from_columns(
        model.id,
        &model.name,
        model.description,
        model.capacity,
        model.opens_at,
        model.closes_at,
    )
}

// SeaORM error -> port error. `sql_err()` classifies database errors portably.
fn map_error(error: DbErr) -> RepositoryError {
    if let Some(SqlErr::UniqueConstraintViolation(_)) = error.sql_err() {
        return RepositoryError::Conflict("a room with this name already exists".to_string());
    }
    RepositoryError::unexpected("sea-orm operation failed", error)
}

#[async_trait]
impl RoomRepository for SeaOrmRoomRepository {
    async fn find(&self, filter: &RoomFilter, page: PageRequest) -> RepositoryResult<Page<Room>> {
        // Conditions are added only when the filter is set – dynamic queries are natural here.
        let mut query = room_entity::Entity::find();
        if let Some(min) = filter.min_capacity {
            query = query.filter(room_entity::Column::Capacity.gte(to_db_int(min)));
        }
        if let Some(name) = &filter.name {
            query = query.filter(room_entity::Column::Name.ilike(like_pattern(name)));
        }

        // Built-in paginator: runs the COUNT and the LIMIT/OFFSET query. Pages are 0-based.
        let paginator = query
            .order_by_asc(room_entity::Column::Id)
            .paginate(&self.db, u64::from(page.size()));
        let total = paginator.num_items().await.map_err(map_error)?;
        let models = paginator
            .fetch_page(u64::from(page.page() - 1))
            .await
            .map_err(map_error)?;

        Ok(Page {
            items: models
                .into_iter()
                .map(to_domain)
                .collect::<Result<_, _>>()?,
            request: page,
            total,
        })
    }

    async fn find_by_id(&self, id: Uuid) -> RepositoryResult<Option<Room>> {
        room_entity::Entity::find_by_id(id)
            .one(&self.db)
            .await
            .map_err(map_error)?
            .map(to_domain)
            .transpose()
    }

    async fn insert(&self, new_room: NewRoom) -> RepositoryResult<Room> {
        // `Set(v)` – value to write; fields left `NotSet` are omitted from the INSERT
        // (database defaults apply, e.g. `created_at`).
        let model = room_entity::ActiveModel {
            id: Set(Uuid::now_v7()),
            name: Set(new_room.name.to_string()),
            description: Set(new_room.description),
            capacity: Set(to_db_int(new_room.capacity)),
            opens_at: Set(new_room.opening_hours.opens_at()),
            closes_at: Set(new_room.opening_hours.closes_at()),
        }
        .insert(&self.db) // INSERT ... RETURNING *
        .await
        .map_err(map_error)?;
        to_domain(model)
    }

    async fn update(&self, id: Uuid, data: NewRoom) -> RepositoryResult<Option<Room>> {
        // `Unchanged(id)` – used in WHERE, not in SET.
        let result = room_entity::ActiveModel {
            id: Unchanged(id),
            name: Set(data.name.to_string()),
            description: Set(data.description),
            capacity: Set(to_db_int(data.capacity)),
            opens_at: Set(data.opening_hours.opens_at()),
            closes_at: Set(data.opening_hours.closes_at()),
        }
        .update(&self.db)
        .await;

        match result {
            Ok(model) => to_domain(model).map(Some),
            // UPDATE matched no row.
            Err(DbErr::RecordNotUpdated) => Ok(None),
            Err(error) => Err(map_error(error)),
        }
    }

    async fn delete(&self, id: Uuid) -> RepositoryResult<bool> {
        let result = room_entity::Entity::delete_by_id(id)
            .exec(&self.db)
            .await
            .map_err(map_error)?;
        Ok(result.rows_affected > 0)
    }
}
