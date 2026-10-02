# 008 – Data transfer objects and mapping between layers

## Goal

Separate the public API contract (DTOs) from domain models and map between them with
the standard conversion traits.

## Key concepts

### Model types per layer

| Layer | Types | Knows serde? |
|-------|-------|--------------|
| api | `RoomRequest`, `RoomQuery`, `RoomResponse`, `BookingStatusDto` ... | yes – defines the JSON contract |
| domain | `Room`, `NewRoom`, `RoomFilter`, `Booking`, `BookingStatus` ... | no |
| infrastructure | DB row types (`RoomRow`, step 015) | sqlx `FromRow` |

```
JSON ──serde──▶ RoomRequest ──From──▶ NewRoom ──service──▶ Room ──From──▶ RoomResponse ──serde──▶ JSON
```

Why not serialize domain models directly:
- API changes only on purpose – renaming a domain field doesn't silently change JSON,
- no accidental leaks (password hash, internal flags, audit fields),
- input DTOs contain only client-settable fields (no `id`, `status`, `createdAt`),
- output can be shaped (derived `durationMinutes`, flattened data, links),
- one domain model can have several representations (summary vs. details, v1 vs. v2).

Cost: more types and boilerplate. For trivial CRUD it may be acceptable to expose domain
types; for business services, separate DTOs pay off quickly.

### Conversion traits

```rust
impl From<Room> for RoomResponse {
    fn from(room: Room) -> Self { Self { id: room.id, name: room.name, ... } }
}

let response = RoomResponse::from(room);
let response: RoomResponse = room.into();            // `Into` comes for free from `From`
let list: Vec<RoomResponse> = rooms.into_iter().map(RoomResponse::from).collect();
let status = query.status.map(Into::into);           // Option<BookingStatusDto> -> Option<BookingStatus>
```

```rust
impl TryFrom<RoomRequest> for NewRoom {          // fallible mapping (step 009)
    type Error = ValidationErrors;
    fn try_from(r: RoomRequest) -> Result<Self, Self::Error> { ... }
}
let new_room: NewRoom = request.try_into()?;
```

Guidelines:
- implement `From`/`TryFrom`, never `Into`/`TryInto` directly,
- put conversions next to the DTO (API layer depends on domain, not the other way round),
- consume the source (`from(room: Room)`) to move fields instead of cloning,
- exhaustive `match` in enum mappings – adding a domain variant breaks compilation until the API decides how to expose it.

Mapping crates (`o2o`, `derive_more`, ...) can generate trivial conversions; hand-written
`From` impls are explicit and usually short enough.

### Naming conventions used

| Suffix | Direction | Example |
|--------|-----------|---------|
| `*Request` | body in | `RoomRequest`, `CreateBookingRequest` |
| `*Query` | query string in | `RoomQuery`, `BookingQuery` |
| `*Response` | body out | `RoomResponse`, `BookingResponse` |
| `*Dto` | shared value types | `BookingStatusDto` |

## What changed in this branch

- `src/api/dto/{mod,rooms,bookings}.rs` (new) – DTOs, `From` conversions, test of the JSON shape
- `src/api/serde_formats.rs` – moved from the crate root (it's an API concern now)
- `src/domain/room.rs`, `src/domain/booking.rs` – serde removed; `Booking::duration()`
- `src/api/rooms.rs`, `src/api/bookings.rs` – handlers map DTO ↔ domain
- new field in responses: `durationMinutes`

## Try it

```bash
cargo test
cargo run
B=localhost:3000/api/v1
ROOM=$(curl -s "$B/rooms?name=blue" | jq -r '.[0].id')
curl -X POST $B/bookings -H 'content-type: application/json' \
  -d "{\"roomId\":\"$ROOM\",\"userId\":\"0199a3f0-0000-7000-8000-000000000007\",\"startTime\":\"2026-10-05T09:00:00Z\",\"endTime\":\"2026-10-05T10:30:00Z\"}"
# -> "durationMinutes": 90
```

## Exercises

1. Add `RoomSummaryResponse { id, name }` and return it from `GET /rooms?view=summary`.
2. Add an internal field `notes: String` to the domain `Room` and confirm it never appears in JSON.
3. Add a new domain status `Expired` and follow compiler errors to update the mapping.
