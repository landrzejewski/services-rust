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

### Transaction + TxManager – transactions without leaking SQL into the domain

Two generic domain ports (`src/domain/transaction.rs`), used through `dyn`:

```rust
#[async_trait] pub trait TxManager: Send + Sync {
    async fn begin(&self) -> RepositoryResult<Box<dyn Transaction>>;
}
#[async_trait] pub trait Transaction: Send {
    async fn commit(self: Box<Self>) -> RepositoryResult<()>;
    async fn rollback(self: Box<Self>) -> RepositoryResult<()>;   // drop without commit = rollback
    fn as_any_mut(&mut self) -> &mut dyn Any;                     // for adapters only
}
```

Repository methods that must run inside a transaction take `&mut dyn Transaction`; the service
decides where the transaction begins and ends:

```rust
// BookingService::create_booking
let mut tx = self.tx_manager.begin().await?;
let room = self.rooms.find_by_id_for_update(tx.as_mut(), id).await?.ok_or_else(...)?;
self.check_booking_fits_room(&new, &room)?;        // `?` -> early return -> drop -> ROLLBACK
self.bookings.lock_user(tx.as_mut(), new.user_id).await?;
self.check_user_limit(tx.as_mut(), &new).await?;
self.check_no_overlap(tx.as_mut(), &new).await?;
let booking = self.bookings.insert(tx.as_mut(), new).await?;
tx.commit().await?;

// RoomService::delete_room – same room lock, so deletion and booking can't interleave
let mut tx = self.tx_manager.begin().await?;
self.repository.find_by_id_for_update(tx.as_mut(), id).await?;   // None -> 404
self.bookings.count_active_by_room(tx.as_mut(), id, now).await?; // > 0 -> 409
self.repository.delete(tx.as_mut(), id).await?;
tx.commit().await?;
```

The adapter turns `&mut dyn Transaction` back into its own type:

```rust
// postgres/transaction.rs
pub(super) fn connection(tx: &mut dyn Transaction) -> RepositoryResult<&mut PgConnection> {
    tx.as_any_mut().downcast_mut::<PostgresTransaction>()
        .map(|t| &mut *t.tx)
        .ok_or_else(|| RepositoryError::Unexpected { .. })
}
sqlx::query!(...).fetch_one(connection(tx)?).await?;
```

Adapters: `PostgresTxManager` (real transaction on one pooled connection) and `InMemoryTxManager`
(global `tokio::sync::Mutex`, writes deferred until commit) for unit tests.

Trade-off of `dyn`: services stay free of type parameters and the implementation is chosen at
runtime in `app.rs`, but the compiler can't check that the transaction and the repository belong
to the same storage – a mismatch is detected by the downcast at runtime
(`RepositoryError::Unexpected`). A generic design (`TxManager` with an associated `Tx` type,
services generic over it) moves that check to compile time.

Other approaches: pass `&mut PgConnection`/`Transaction` through service methods (simple, but
couples the domain to sqlx); a closure-based `tx_manager.run(|tx| async { ... })` API (harder with
async lifetimes); optimistic locking with a `version` column for updates
(`UPDATE ... WHERE id = $1 AND version = $2`).

### Keep transactions short

A transaction holds a pooled connection and its locks. No HTTP calls, no long computations, no
waiting for user input inside it.

## What changed in this branch

- `migrations/*_bookings_no_overlap.sql` (new) – `btree_gist`, exclusion constraint
- `src/domain/transaction.rs` (new) – `Transaction`, `TxManager`
- `src/domain/repositories.rs` – transactional methods take `&mut dyn Transaction`
  (`find_by_id_for_update`, `delete`, `insert`, `lock_user`, `find_active_overlapping`, `count_active_by_*`)
- `src/domain/booking_service.rs` – `create_booking` in a transaction; checks take `&mut dyn Transaction`
- `src/domain/room_service.rs` – `delete_room` in a transaction (room lock + count + delete)
- `src/infrastructure/postgres/transaction.rs` (new) – `PostgresTxManager`, downcast to `PgConnection`
- `src/infrastructure/postgres/{booking,room}_repository.rs` – `FOR UPDATE`, advisory lock, queries on the transaction's connection
- `src/infrastructure/postgres/mod.rs` – `23P01` → `Conflict`
- `src/infrastructure/memory/transaction.rs` (new) – `InMemoryTxManager` for tests
- `src/app.rs` – one `Arc<dyn TxManager>` injected into both services; `.sqlx/` – regenerated

## Try it

```bash
docker compose up -d && cargo run
B=localhost:3000/api/v1; R=0199a3f0-0000-7000-8000-000000000003

# 20 parallel requests for the same slot -> exactly one 201, the rest 409
for i in $(seq 1 20); do U=$(printf '0199a3f0-0000-7000-9000-%012d' $i)
  curl -s -o /dev/null -w '%{http_code}\n' -X POST $B/bookings -H 'content-type: application/json' \
  -d "{\"roomId\":\"$R\",\"userId\":\"$U\",\"startTime\":\"2026-10-07T09:00:00Z\",\"endTime\":\"2026-10-07T10:00:00Z\"}" &
done | sort | uniq -c

# a room with an upcoming booking can't be deleted -> 409
curl -s -X DELETE $B/rooms/$R

# the constraint protects even raw SQL
docker compose exec postgres psql -U booking -d booking -c \
  "insert into bookings values (gen_random_uuid(), '$R', gen_random_uuid(), '2026-10-07T09:30Z', '2026-10-07T10:30Z', 1, 'ACTIVE')"
```

## Exercises

1. In `create_booking` replace `find_by_id_for_update` with plain `find_by_id` and repeat the parallel test – which mechanism now produces the 409s and what is the message?
2. Make cancellation race-free with a conditional update: `UPDATE ... SET status = 'CANCELLED' WHERE id = $1 AND status = 'ACTIVE'`.
3. Switch the transaction to SERIALIZABLE, remove both locks and implement a retry loop for SQLSTATE `40001`.
