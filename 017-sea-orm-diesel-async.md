# 017 – Database access: sqlx, SeaORM, diesel-async – selection criteria

## Goal

Implement the same `RoomRepository` port with SeaORM and with Diesel (diesel-async), compare the
three approaches, and know when to pick which.

## Key concepts

### Three styles

| | sqlx | SeaORM | Diesel + diesel-async |
|---|---|---|---|
| Style | SQL strings + mapping | dynamic ORM (entities, ActiveModel) on top of sqlx + SeaQuery | typed query builder / ORM, schema in Rust |
| Async | native | native | diesel sync; `diesel-async` adds async connections & pools |
| Compile-time checks | `query!` macros check SQL against a live DB / `.sqlx` cache | entity types only; queries checked at runtime | full type-checking of queries against `schema.rs`, no DB needed |
| Dynamic queries | `QueryBuilder` or `$1 IS NULL OR ...` tricks | natural (`if` + `.filter()`) | `into_boxed()` + `.filter()` |
| Relations / eager loading | manual JOINs | built-in (`find_with_related`, loaders) | `belonging_to`, joins via `joinable!` |
| Migrations | `sqlx migrate` (SQL files) | `sea-orm-migration` (Rust or SQL) | `diesel migration` (SQL files), `diesel print-schema` |
| Code generation | – | entities from DB (`sea-orm-cli generate entity`) | `schema.rs` from DB |
| Raw SQL | primary interface | `Statement::from_sql_and_values` | `sql_query` |
| Learning curve | low (know SQL) | medium | highest (complex generic types/errors) |
| Pool | `PgPool` | reuses sqlx pool | deadpool / bb8 / mobc |
| Databases | PG, MySQL, SQLite | PG, MySQL, SQLite | PG, MySQL, SQLite (+ community backends) |

### Selection criteria

- **Team knows SQL well, queries are domain-specific (locks, CTEs, window functions, `EXCLUDE`)** → sqlx.
  Full control, no abstraction leaks, compile-time verified SQL. Default choice for services.
- **Much CRUD, many entities and relations, admin-style APIs, dynamic filters** → SeaORM.
  Less boilerplate, generated entities, works with an existing sqlx pool.
- **Maximum compile-time safety, complex query composition, long-lived codebase** → Diesel.
  Strongest guarantees without a database at build time; async via `diesel-async`.
- Mixed usage is possible (as here): ORM for simple CRUD, sqlx for critical transactional paths.
- Consider also: build-time requirements (sqlx macros need DB or `.sqlx`), compile times (Diesel
  generics, SeaORM macros), error messages, maintenance activity, transaction API ergonomics.

### Same port, three adapters

```
              RoomRepository (domain trait)
        ┌──────────────┼──────────────────┐
PostgresRoomRepository  SeaOrmRoomRepository  DieselRoomRepository
   (sqlx, default)      (feature orm-sea)      (feature orm-diesel)
```

Chosen at startup:

```toml
[storage]
room_repository = "sqlx"   # | "sea-orm" | "diesel"
```

```rust
#[allow(unreachable_patterns)]
let repository: Arc<dyn RoomRepository> = match kind {
    RoomRepositoryKind::Sqlx => Arc::new(PostgresRoomRepository::new(db.clone())),
    #[cfg(feature = "orm-sea")]
    RoomRepositoryKind::SeaOrm => Arc::new(SeaOrmRoomRepository::new(db.clone())),
    #[cfg(feature = "orm-diesel")]
    RoomRepositoryKind::Diesel => Arc::new(DieselRoomRepository::connect(&settings.database)?),
    other => anyhow::bail!("{other:?} is not compiled in"),
};
```

### Cargo features for optional implementations

```toml
[dependencies]
sea-orm = { version = "2", optional = true, default-features = false, features = [...] }
diesel = { version = "2", optional = true, default-features = false, features = ["postgres_backend", "uuid", "chrono"] }
diesel-async = { version = "0.9", optional = true, features = ["postgres", "deadpool"] }

[features]
orm-sea = ["dep:sea-orm"]
orm-diesel = ["dep:diesel", "dep:diesel-async"]
```

```rust
#[cfg(feature = "orm-sea")]
pub mod sea_orm;
```

Disabled features → the code and dependencies are not compiled at all. Check every combination in CI:
`cargo clippy --all-targets`, `--features orm-sea`, `--features orm-diesel`, `--all-features`.

### SeaORM essentials

```rust
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "rooms")]
pub struct Model { #[sea_orm(primary_key, auto_increment = false)] pub id: Uuid, pub name: String, ... }

let page = room::Entity::find()
    .filter(room::Column::Capacity.gte(5))
    .order_by_asc(room::Column::Id)
    .paginate(&db, 20);                       // .num_items(), .fetch_page(0)

room::ActiveModel { id: Set(id), name: Set(name), ..Default::default() }.insert(&db).await?;
room::ActiveModel { id: Unchanged(id), name: Set(new_name), .. }.update(&db).await?;   // Err(RecordNotUpdated) if missing
```

`SqlxPostgresConnector::from_sqlx_postgres_pool(pool)` shares the application's sqlx pool.

### Diesel essentials

```rust
diesel::table! { rooms (id) { id -> Uuid, name -> Text, description -> Nullable<Text>, ... } }

#[derive(Queryable, Selectable)] #[diesel(table_name = rooms)] struct RoomRecord { ... }
#[derive(Insertable, AsChangeset)] #[diesel(treat_none_as_null = true)] struct RoomValues<'a> { ... }

let mut query = rooms::table.into_boxed();               // dynamic filters
if let Some(min) = min { query = query.filter(rooms::capacity.ge(min)); }
query.select(RoomRecord::as_select()).load(&mut conn).await?;   // diesel_async::RunQueryDsl

rooms::table.find(id).first(&mut conn).await.optional()?;        // NotFound -> None
diesel::update(rooms::table.find(id)).set(values).returning(RoomRecord::as_returning()).get_result(&mut conn).await
```

Pitfall: `AsChangeset` skips `None` fields by default – for PUT semantics use `treat_none_as_null = true`.

## What changed in this branch

- `Cargo.toml` – optional `sea-orm`, `diesel`, `diesel-async`; features `orm-sea`, `orm-diesel`
- `src/infrastructure/sea_orm/{mod,room_entity}.rs` (new) – `SeaOrmRoomRepository`
- `src/infrastructure/diesel/{mod,schema}.rs` (new) – `DieselRoomRepository` with deadpool
- `src/infrastructure/postgres/*` – shared helpers (`room_from_columns`, `to_db_int`, `like_pattern`)
- `src/config.rs`, `config/default.toml` – `[storage] room_repository`
- `src/app.rs` – implementation selected at startup

Bookings and the transactional booking path stay on sqlx.

## Try it

```bash
cargo build --all-features
APP_STORAGE__ROOM_REPOSITORY=sea-orm cargo run --features orm-sea
APP_STORAGE__ROOM_REPOSITORY=diesel  cargo run --features orm-diesel
curl "localhost:3000/api/v1/rooms?minCapacity=5&page=1&size=2"     # same results for all three
APP_STORAGE__ROOM_REPOSITORY=diesel cargo run                       # error: not compiled in
```

## Exercises

1. Implement `BookingRepository::find` with SeaORM (entity for `bookings`, enum mapping for `status`).
2. Add `RUST_LOG=sea_orm=debug` / `diesel` logging and compare the generated SQL with the sqlx query.
3. Write a table comparing compile time (`cargo build --timings`) of the three feature sets.
