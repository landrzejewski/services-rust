# 015 – Persisting and accessing data with sqlx

## Goal

Implement the repository ports with PostgreSQL using sqlx: compile-time checked SQL, mapping rows
to domain types, error translation, pagination.

## Key concepts

### Query APIs

| API | Checked at compile time | Result |
|-----|------------------------|--------|
| `sqlx::query!("...", args)` | yes | anonymous struct per row / `execute` |
| `sqlx::query_as!(Row, "...", args)` | yes | `Row` (fields matched by column name) |
| `sqlx::query_scalar!("...")` | yes | single value |
| `sqlx::query("...").bind(x)` | no | `PgRow`, read with `row.try_get("col")` |
| `sqlx::query_as::<_, Row>("...")` | no | `Row: FromRow` (derive) |
| `sqlx::QueryBuilder` | no | dynamic SQL (variable filters, bulk insert) |

Execution: `fetch_one` (exactly 1), `fetch_optional` (0..1), `fetch_all` (Vec), `fetch` (stream),
`execute` (rows affected). Executor: `&PgPool`, `&mut PgConnection` or `&mut Transaction` (step 016).

### Compile-time checking

Macros connect to `DATABASE_URL` while compiling, prepare each statement and verify:
syntax, table/column names, parameter and result types, nullability.

```rust
let rows = sqlx::query_as!(RoomRow,
    r#"SELECT id, name, capacity FROM rooms WHERE ($1::int4 IS NULL OR capacity >= $1)"#,
    min_capacity)            // Option<i32> -> NULL disables the filter
    .fetch_all(&pool).await?;
```

Nullability overrides in column aliases: `count(*) AS "total!"` (not null), `AS "x?"` (nullable),
`AS "id: MyType"` (custom type).

### Offline mode – `.sqlx/`

```bash
cargo sqlx prepare -- --all-targets   # writes .sqlx/query-*.json (commit them!)
SQLX_OFFLINE=true cargo build         # builds without a database (CI, Docker)
cargo sqlx prepare --check            # CI: fail if metadata is stale
```

Without `DATABASE_URL`, macros use `.sqlx/` automatically.

### Type mapping (PostgreSQL ↔ Rust)

| PostgreSQL | Rust |
|------------|------|
| `UUID` | `uuid::Uuid` (feature `uuid`) |
| `TEXT`, `VARCHAR` | `String` / `&str` |
| `INTEGER` / `BIGINT` | `i32` / `i64` (no unsigned types in PostgreSQL) |
| `TIMESTAMPTZ` | `chrono::DateTime<Utc>` (feature `chrono`) |
| `TIME` / `DATE` | `NaiveTime` / `NaiveDate` |
| `BOOLEAN` | `bool` |
| `JSONB` | `serde_json::Value`, `sqlx::types::Json<T>` |
| nullable column | `Option<T>` |

### Row types and mapping

```
PostgreSQL row ──query_as!──▶ RoomRow (i32, String) ──TryFrom──▶ Room (u32, RoomName, OpeningHours)
```

Row types belong to infrastructure. The conversion is fallible: invalid data in the database
becomes `RepositoryError::Unexpected` instead of an invalid domain object.

`Option<Row>` → `Option<Room>` with a fallible map: `row.map(Room::try_from).transpose()`.

### Error translation

```rust
impl From<sqlx::Error> for RepositoryError {
    fn from(error: sqlx::Error) -> Self {
        if let sqlx::Error::Database(db) = &error && db.is_unique_violation() {
            return RepositoryError::Conflict("a room with this name already exists".into());
        }
        RepositoryError::unexpected("database operation failed", error)
    }
}
```

`RepositoryError::Conflict` → `DomainError::Conflict` → 409; everything else → 500 (logged).
Useful helpers: `is_unique_violation()`, `is_foreign_key_violation()`, `is_check_violation()`,
`constraint()`, `code()` (SQLSTATE).

### Pagination

Offset pagination: `?page=2&size=20` → `LIMIT 20 OFFSET 20` + `count(*)` for the total.

```json
{ "items": [...], "page": 2, "size": 20, "totalItems": 42, "totalPages": 3 }
```

- Always `ORDER BY` a unique key (stable pages).
- Limit `size` (here 1..=100).
- Large tables: `OFFSET` scans skipped rows → **keyset pagination** (`WHERE id > $last ORDER BY id LIMIT n`,
  cursor returned to the client).

### Safety notes

- Bind parameters (`$1`) – values never become part of the SQL text → no SQL injection.
- `LIKE`/`ILIKE` patterns: escape `%`, `_`, `\` in user input (`like_pattern`).

## What changed in this branch

- `src/infrastructure/postgres/room_repository.rs`, `booking_repository.rs` (new) – sqlx repositories
- `src/infrastructure/postgres/mod.rs` – `From<sqlx::Error>`, `like_pattern`
- `src/domain/repositories.rs` – `RepositoryError` enum (`Conflict`, `Unexpected`), paged `find`
- `src/domain/error.rs` – manual `From<RepositoryError>`
- `src/domain/pagination.rs` (new), `src/api/dto/pagination.rs` (new) – `PageRequest`, `Page`, `PageResponse`
- DTOs/handlers/services – paging parameters, page envelope responses
- `src/app.rs` – PostgreSQL repositories wired in
- `migrations/*_seed_rooms.sql` – sample rooms with fixed ids
- `.sqlx/` – offline query metadata; `Cargo.toml` – `sqlx-macros` optimized in dev

## Try it

```bash
docker compose up -d
cargo run
B=localhost:3000/api/v1
curl "$B/rooms?size=2&page=2"
curl "$B/rooms?name=_"                       # literal underscore, no match
curl -X POST $B/rooms -H 'content-type: application/json' -d '{"name":"blue ROOM","capacity":3}'   # 409
docker compose exec postgres psql -U booking -d booking -c 'select * from bookings'
docker compose down -v && docker compose up -d   # reset the database
```

## Exercises

1. Add `GET /api/v1/rooms?sort=capacity` – why can't the column name be a bind parameter? Use `QueryBuilder` or a whitelist.
2. Implement keyset pagination for bookings (`?after=<id>`).
3. Add `rooms.floor` (migration + row + domain + DTO) and follow compile errors from the `query_as!` macros.
