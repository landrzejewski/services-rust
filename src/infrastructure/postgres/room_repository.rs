use async_trait::async_trait;
use chrono::NaiveTime;
use sqlx::PgPool;
use uuid::Uuid;

use super::{like_pattern, transaction::connection};
use crate::domain::{
    pagination::{Page, PageRequest},
    repositories::{RepositoryError, RepositoryResult, RoomRepository},
    room::{NewRoom, OpeningHours, Room, RoomFilter, RoomName},
    transaction::Transaction,
};

/// `RoomRepository` backed by PostgreSQL.
pub struct PostgresRoomRepository {
    // `PgPool` is a cheap handle (Arc inside) – every query borrows a pooled connection.
    pool: PgPool,
}

impl PostgresRoomRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Row type – mirrors the table, uses database-friendly types (`i32` for INTEGER, `String`).
/// Infrastructure model, separate from the domain `Room` (like DTOs in the API layer).
struct RoomRow {
    id: Uuid,
    name: String,
    description: Option<String>,
    capacity: i32,
    opens_at: NaiveTime,
    closes_at: NaiveTime,
}

// Row -> domain. Fallible: the database could contain data violating domain invariants
// (manual edits, older versions of the app). Such data is reported as an unexpected error
// instead of silently creating an invalid domain object.
impl TryFrom<RoomRow> for Room {
    type Error = RepositoryError;

    fn try_from(row: RoomRow) -> Result<Self, Self::Error> {
        Ok(Room {
            id: row.id,
            name: RoomName::parse(&row.name)
                .map_err(|e| RepositoryError::unexpected("invalid room row", e))?,
            description: row.description,
            capacity: u32::try_from(row.capacity)
                .map_err(|e| RepositoryError::unexpected("invalid room capacity", e))?,
            opening_hours: OpeningHours::new(row.opens_at, row.closes_at)
                .map_err(|e| RepositoryError::unexpected("invalid room row", e))?,
        })
    }
}

/// Domain `u32` -> PostgreSQL INTEGER (`i32`). Values are validated to be small anyway.
fn to_db_int(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

#[async_trait]
impl RoomRepository for PostgresRoomRepository {
    async fn find(&self, filter: &RoomFilter, page: PageRequest) -> RepositoryResult<Page<Room>> {
        let min_capacity = filter.min_capacity.map(to_db_int);
        let name = filter.name.as_deref().map(like_pattern);

        // `query_as!` – compile-time checked query:
        // - at build time the macro sends the SQL to the database (DATABASE_URL) or reads the
        //   cached metadata from `.sqlx/` (offline mode) and verifies syntax, column names and types,
        // - generates code mapping columns to `RoomRow` fields by name.
        // A typo in a column or a type mismatch is a COMPILE error, not a runtime one.
        //
        // Optional filters in a static query: `$1 IS NULL OR ...` – a NULL parameter disables the
        // condition. `$1::int4` gives PostgreSQL the parameter type it can't infer from `IS NULL`.
        // Parameters are always sent separately from the SQL text (bind parameters) –
        // SQL injection is impossible.
        let rows = sqlx::query_as!(
            RoomRow,
            r#"
            SELECT id, name, description, capacity, opens_at, closes_at
            FROM rooms
            WHERE ($1::int4 IS NULL OR capacity >= $1)
              AND ($2::text IS NULL OR name ILIKE $2)
            ORDER BY id
            LIMIT $3 OFFSET $4
            "#,
            min_capacity,
            name,
            i64::from(page.size()),
            i64::try_from(page.offset()).unwrap_or(i64::MAX),
        )
        // `fetch_all` – all rows into a Vec. Others: `fetch_one` (exactly one, error otherwise),
        // `fetch_optional` (0 or 1), `fetch` (async stream for large results).
        .fetch_all(&self.pool)
        .await?;

        // `"total!"` – the `!` suffix overrides nullability inference: `count(*)` is never NULL,
        // so the field is `i64` instead of `Option<i64>`.
        let total = sqlx::query_scalar!(
            r#"
            SELECT count(*) AS "total!"
            FROM rooms
            WHERE ($1::int4 IS NULL OR capacity >= $1)
              AND ($2::text IS NULL OR name ILIKE $2)
            "#,
            min_capacity,
            name,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(Page {
            // `collect::<Result<Vec<_>, _>>()` – stops at the first failed conversion.
            items: rows
                .into_iter()
                .map(Room::try_from)
                .collect::<Result<_, _>>()?,
            request: page,
            total: u64::try_from(total).unwrap_or_default(),
        })
    }

    async fn find_by_id(&self, id: Uuid) -> RepositoryResult<Option<Room>> {
        let row = sqlx::query_as!(
            RoomRow,
            "SELECT id, name, description, capacity, opens_at, closes_at FROM rooms WHERE id = $1",
            id
        )
        .fetch_optional(&self.pool)
        .await?;

        // `Option<RoomRow>` -> `Option<Room>` with a fallible conversion:
        // `map` + `transpose` turns `Option<Result<T, E>>` into `Result<Option<T>, E>`.
        row.map(Room::try_from).transpose()
    }

    async fn insert(&self, new_room: NewRoom) -> RepositoryResult<Room> {
        // `RETURNING` – the inserted row comes back in the same round trip.
        let row = sqlx::query_as!(
            RoomRow,
            r#"
            INSERT INTO rooms (id, name, description, capacity, opens_at, closes_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id, name, description, capacity, opens_at, closes_at
            "#,
            Uuid::now_v7(),
            new_room.name.as_str(),
            new_room.description,
            to_db_int(new_room.capacity),
            new_room.opening_hours.opens_at(),
            new_room.opening_hours.closes_at(),
        )
        .fetch_one(&self.pool)
        // A duplicate name violates `rooms_name_idx` -> `RepositoryError::Conflict` (see `From`).
        .await?;

        row.try_into()
    }

    async fn update(&self, id: Uuid, data: NewRoom) -> RepositoryResult<Option<Room>> {
        let row = sqlx::query_as!(
            RoomRow,
            r#"
            UPDATE rooms
            SET name = $2, description = $3, capacity = $4, opens_at = $5, closes_at = $6
            WHERE id = $1
            RETURNING id, name, description, capacity, opens_at, closes_at
            "#,
            id,
            data.name.as_str(),
            data.description,
            to_db_int(data.capacity),
            data.opening_hours.opens_at(),
            data.opening_hours.closes_at(),
        )
        .fetch_optional(&self.pool)
        .await?;

        row.map(Room::try_from).transpose()
    }

    /// `FOR UPDATE` – other transactions trying to lock the same row WAIT until this one commits
    /// or rolls back. Plain reads (without `FOR UPDATE`) are not blocked (MVCC).
    async fn find_by_id_for_update(
        &self,
        tx: &mut dyn Transaction,
        id: Uuid,
    ) -> RepositoryResult<Option<Room>> {
        let row = sqlx::query_as!(
            RoomRow,
            r#"
            SELECT id, name, description, capacity, opens_at, closes_at
            FROM rooms WHERE id = $1
            FOR UPDATE
            "#,
            id
        )
        // The transaction's connection instead of `&self.pool`: the lock lives as long as the
        // transaction, not just this one statement.
        .fetch_optional(connection(tx)?)
        .await?;
        row.map(Room::try_from).transpose()
    }

    async fn delete(&self, tx: &mut dyn Transaction, id: Uuid) -> RepositoryResult<bool> {
        // `query!` (no `_as`) for statements without a result mapping; `execute` returns
        // the number of affected rows.
        let result = sqlx::query!("DELETE FROM rooms WHERE id = $1", id)
            .execute(connection(tx)?)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
