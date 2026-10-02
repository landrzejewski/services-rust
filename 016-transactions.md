# 016 – Transactions

## Goal

Make the multi-step "create booking" use case correct under concurrency: one transaction,
explicit locks, and a database constraint as the final guarantee.

## Key concepts

### The problem (step 013)

```
request A: check overlap (none) ─────────────── insert ✔
request B:        check overlap (none) ─────────────── insert ✔   → double booking
```

Each statement is atomic, the sequence is not. Fix = the reads and the write must happen in one
transaction **and** concurrent transactions must not interleave in a harmful way.

### ACID in one table

| | Meaning |
|---|---|
| Atomicity | all statements commit or none (rollback) |
| Consistency | constraints hold after every transaction |
| Isolation | concurrent transactions don't see each other's partial work (level-dependent) |
| Durability | committed data survives crashes |

### Isolation levels (PostgreSQL)

| Level | Prevents | Notes |
|-------|----------|-------|
| READ COMMITTED (default) | dirty reads | each statement sees data committed before it started → check-then-insert races remain |
| REPEATABLE READ | + non-repeatable reads | snapshot per transaction; write conflicts → error 40001 |
| SERIALIZABLE | + all anomalies (write skew) | may abort with 40001 → **retry required** |

```rust
let mut tx = pool.begin().await?;
sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE").execute(&mut *tx).await?;
```

This project stays on READ COMMITTED and uses explicit locks – no retry logic needed.

### Transactions in sqlx

```rust
let mut tx = pool.begin().await?;                  // BEGIN, holds one connection
sqlx::query!(...).execute(&mut *tx).await?;        // &mut *tx = &mut PgConnection (executor)
sqlx::query!(...).fetch_one(&mut *tx).await?;
tx.commit().await?;                                // COMMIT
// dropped without commit -> ROLLBACK
```

Write queries once, run them on a pool or inside a transaction:

```rust
pub async fn insert_booking(executor: impl PgExecutor<'_>, b: NewBooking) -> RepositoryResult<Booking>
insert_booking(&pool, b).await       // standalone
insert_booking(&mut *tx, b).await    // inside the transaction
```

### Locks used

| Lock | SQL | Protects |
|------|-----|----------|
| row lock | `SELECT ... FROM rooms WHERE id = $1 FOR UPDATE` | bookings of one room are created one at a time → overlap check is race-free |
| advisory lock | `SELECT pg_advisory_xact_lock(hashtextextended($1::text, 0))` | per-user limit across rooms |

Both are released at COMMIT/ROLLBACK. Acquire locks in a consistent order (room → user) to avoid
deadlocks. Variants: `FOR UPDATE NOWAIT` (fail instead of wait), `SKIP LOCKED` (job queues),
`FOR SHARE`.

### Constraint as the last line of defense

```sql
CREATE EXTENSION IF NOT EXISTS btree_gist;
ALTER TABLE bookings ADD CONSTRAINT bookings_no_overlap
    EXCLUDE USING gist (room_id WITH =, tstzrange(start_time, end_time, '[)') WITH &&)
    WHERE (status = 'ACTIVE');
```

Holds for every writer (other services, scripts, bugs). Violation → SQLSTATE `23P01` →
`RepositoryError::Conflict` → 409. Application checks still matter: they give precise error
messages and enforce rules a constraint can't express (opening hours, limits).

### Unit of work – transactions without leaking SQL into the domain

```rust
// domain port
#[async_trait] pub trait BookingUnitOfWork: Send + Sync {
    async fn begin(&self) -> RepositoryResult<Box<dyn BookingTransaction>>;
}
#[async_trait] pub trait BookingTransaction: Send {
    async fn lock_room(&mut self, room_id: Uuid) -> RepositoryResult<Option<Room>>;
    async fn lock_user(&mut self, user_id: Uuid) -> RepositoryResult<()>;
    async fn find_active_overlapping(&mut self, ...) -> ...;
    async fn insert_booking(&mut self, ...) -> ...;
    async fn commit(self: Box<Self>) -> RepositoryResult<()>;
}

// service
let mut tx = self.unit_of_work.begin().await?;
let room = tx.lock_room(id).await?.ok_or_else(...)?;
self.check_booking_fits_room(&new, &room)?;        // `?` -> early return -> drop -> ROLLBACK
tx.lock_user(new.user_id).await?;
self.check_user_limit(tx.as_mut(), &new).await?;
self.check_no_overlap(tx.as_mut(), &new).await?;
let booking = tx.insert_booking(new).await?;
tx.commit().await?;
```

Adapters: `PostgresBookingUnitOfWork` (real transaction) and `InMemoryBookingUnitOfWork`
(global `tokio::sync::Mutex`, buffered inserts) for unit tests.

Other approaches: pass `&mut PgConnection`/`Transaction` through service methods (simple, but
couples the domain to sqlx); a closure-based `uow.run(|tx| async { ... })` API (harder with async
lifetimes); optimistic locking with a `version` column for updates (`UPDATE ... WHERE id = $1 AND version = $2`).

### Keep transactions short

A transaction holds a pooled connection and its locks. No HTTP calls, no long computations, no
waiting for user input inside it.

## What changed in this branch

- `migrations/*_bookings_no_overlap.sql` (new) – `btree_gist`, exclusion constraint
- `src/domain/repositories.rs` – `BookingUnitOfWork`, `BookingTransaction`
- `src/domain/booking_service.rs` – `create_booking` in a transaction; checks take `&mut dyn BookingTransaction`
- `src/infrastructure/postgres/unit_of_work.rs` (new) – transaction, `FOR UPDATE`, advisory lock
- `src/infrastructure/postgres/{booking,room}_repository.rs` – executor-generic query functions
- `src/infrastructure/postgres/mod.rs` – `23P01` → `Conflict`
- `src/infrastructure/memory/unit_of_work.rs` (new) – in-memory unit of work for tests
- `src/app.rs` – wiring; `.sqlx/` – regenerated

## Try it

```bash
docker compose up -d && cargo run
B=localhost:3000/api/v1; R=0199a3f0-0000-7000-8000-000000000003

# 20 parallel requests for the same slot -> exactly one 201, the rest 409
for i in $(seq 1 20); do U=$(printf '0199a3f0-0000-7000-9000-%012d' $i)
  curl -s -o /dev/null -w '%{http_code}\n' -X POST $B/bookings -H 'content-type: application/json' \
  -d "{\"roomId\":\"$R\",\"userId\":\"$U\",\"startTime\":\"2026-10-07T09:00:00Z\",\"endTime\":\"2026-10-07T10:00:00Z\"}" &
done | sort | uniq -c

# the constraint protects even raw SQL
docker compose exec postgres psql -U booking -d booking -c \
  "insert into bookings values (gen_random_uuid(), '$R', gen_random_uuid(), '2026-10-07T09:30Z', '2026-10-07T10:30Z', 1, 'ACTIVE')"
```

## Exercises

1. Remove `lock_room` from `create_booking` and repeat the parallel test – which mechanism now produces the 409s and what is the message?
2. Make cancellation race-free with a conditional update: `UPDATE ... SET status = 'CANCELLED' WHERE id = $1 AND status = 'ACTIVE'`.
3. Switch the transaction to SERIALIZABLE, remove both locks and implement a retry loop for SQLSTATE `40001`.
