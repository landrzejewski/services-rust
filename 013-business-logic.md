# 013 – Implementing business logic

## Goal

Implement the booking rules in the domain layer: explicit, testable, independent of HTTP and
storage, with clear error semantics.

## Key concepts

### Where logic lives

| Element | Responsibility | Example |
|---------|----------------|---------|
| Value object | invariant of a single value | `TimeRange` (start < end), `OpeningHours` |
| Entity | own state transitions | `Booking::cancel(now)` |
| Domain service | rules needing other data / repositories | overlap check, per-user limit |
| Policy | configurable business parameters | `BookingPolicy { max_active_bookings_per_user, max_duration }` |
| Handler | none – translation only | |

Keep handlers thin; keep rules out of repositories (queries yes, decisions no).

### Booking rules

| # | Rule | Error | HTTP |
|---|------|-------|------|
| 1 | room exists | `Invalid` (`roomId`) | 422 |
| 2 | attendees ≤ capacity | `RuleViolated("booking.capacity_exceeded")` | 422 |
| 3 | starts in the future | `RuleViolated("booking.start_in_past")` | 422 |
| 4 | duration ≤ policy | `RuleViolated("booking.too_long")` | 422 |
| 5 | inside opening hours, single day | `RuleViolated("booking.outside_opening_hours")` | 422 |
| 6 | user active bookings < limit | `RuleViolated("booking.user_limit_reached")` | 422 |
| 7 | no overlap with active bookings of the room | `Conflict` | 409 |
| – | cancel: only active | `Conflict` | 409 |
| – | cancel: not started yet | `RuleViolated("booking.already_started")` | 422 |
| – | delete room: no upcoming bookings | `Conflict` | 409 |

422 vs 409: 422 – the request itself is not acceptable under the rules; 409 – it collides
with the current state of a resource (may succeed later / for another slot).
`rule` is a stable machine-readable id returned in the problem (`"rule": "booking.too_long"`).

### Overlap of half-open intervals

`[s1, e1)` and `[s2, e2)` overlap ⇔ `s1 < e2 && s2 < e1`. Back-to-back bookings (`9–10`, `10–11`) are allowed.

### Clock abstraction

```rust
pub trait Clock: Send + Sync { fn now(&self) -> DateTime<Utc>; }
pub struct SystemClock;                 // production
pub struct FixedClock(pub DateTime<Utc>); // tests
```

Rules depending on "now" become deterministic in tests. Same idea for random ids, external
calendars, etc.: inject what you don't control.

### Configuration → policy

```toml
[booking]
max_active_bookings_per_user = 3
max_duration_minutes = 480
```

The composition root converts `Settings` into `BookingPolicy`; the domain never reads config directly.

### Orchestration in the service

```rust
pub async fn create_booking(&self, new: NewBooking) -> DomainResult<Booking> {
    let room = self.rooms.find_by_id(new.room_id).await?.ok_or_else(...)?;
    self.check_booking_fits_room(&new, &room)?;   // pure checks first
    self.check_user_limit(&new).await?;            // then checks needing queries
    self.check_no_overlap(&new).await?;
    Ok(self.bookings.insert(new).await?)
}

pub async fn cancel_booking(&self, id: Uuid) -> DomainResult<Booking> {
    let mut booking = self.get_booking(id).await?;   // load
    booking.cancel(self.clock.now())?;               // behaviour on the entity
    self.bookings.update_status(id, booking.status).await?...   // persist
}
```

**Race condition:** check-then-insert is not atomic – two concurrent requests can both pass the
overlap check. In-memory locks per call don't help. Fixed in step 016 (transaction, row lock,
exclusion constraint).

### Testing rules

Unit tests with in-memory repositories + `FixedClock`; each rule has a test asserting the error
variant and rule id (`matches!(result, Err(DomainError::RuleViolated { rule, .. }) if rule == "...")`).

## What changed in this branch

- `src/domain/booking_service.rs` – rules 1–7, cancellation flow, 7 unit tests
- `src/domain/booking.rs` – `Booking::cancel`, `is_active`
- `src/domain/room.rs` – `OpeningHours::contains`
- `src/domain/room_service.rs` – delete only without upcoming bookings
- `src/domain/clock.rs`, `booking_policy.rs` (new)
- `src/domain/error.rs` – `RuleViolated`, `Conflict`; `src/domain/repositories.rs` – rule queries
- `src/infrastructure/memory/booking_repository.rs` – rule queries
- `src/api/error.rs`, `problem.rs` – 422 business-rule-violation with `rule`, 409 conflict
- `src/config.rs`, `config/default.toml` – `[booking]`; `src/app.rs` – clock and policy wiring

## Try it

```bash
cargo test
cargo run
B=localhost:3000/api/v1
ROOM=$(curl -s "$B/rooms?name=green" | jq -r '.[0].id')
U=0199a3f0-0000-7000-8000-000000000007
bk() { curl -s -X POST $B/bookings -H 'content-type: application/json' \
  -d "{\"roomId\":\"$ROOM\",\"userId\":\"$U\",\"startTime\":\"$1\",\"endTime\":\"$2\",\"attendees\":${3:-1}}"; echo; }
bk 2026-10-05T09:00:00Z 2026-10-05T10:00:00Z        # 201 (use a future date)
bk 2026-10-05T09:30:00Z 2026-10-05T10:30:00Z        # 409 overlap
bk 2026-10-05T11:00:00Z 2026-10-05T12:00:00Z 9      # 422 capacity
bk 2026-10-05T17:00:00Z 2026-10-05T19:00:00Z        # 422 opening hours
curl -X DELETE $B/rooms/$ROOM                        # 409 upcoming bookings
```

## Exercises

1. Add a rule: bookings can be made at most 30 days in advance (policy value + test).
2. Add `minimum notice`: a booking must start at least 15 minutes from now.
3. Allow rooms open 24/7 (`opensAt == closesAt` meaning "always open") – which types/rules change?
