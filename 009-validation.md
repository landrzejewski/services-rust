# 009 – Input validation

## Goal

Reject invalid input at the boundary with clear, field-level error messages, and make invalid
domain states unrepresentable with validated value types.

## Key concepts

### Two levels of validation

| Level | Where | Tool | Purpose |
|-------|-------|------|---------|
| Input validation | API (DTO) | `validator` derive | lengths, ranges, formats, cross-field rules; **all** errors reported with field names |
| Domain invariants | domain types | constructors returning `Result` (newtypes) | guarantee every domain value is valid, regardless of the entry point |
| Business rules | domain services | code + data (step 013) | depend on current state: overlaps, limits, permissions |

Some overlap between the first two is intentional: the API gives good error messages,
the domain doesn't trust any caller.

Parsing the JSON itself (types, required fields, enum values, UUID/time formats) is already done by
serde – a `Uuid` field can never hold an invalid UUID.

### `validator` crate

```rust
#[derive(Deserialize, Validate)]
#[validate(schema(function = "validate_opening_hours", skip_on_field_errors = false))]
pub struct RoomRequest {
    #[validate(length(min = 1, max = 100, message = "must have 1-100 characters"))]
    pub name: String,
    #[validate(length(max = 500))]          // on Option<T>: checked only when Some
    pub description: Option<String>,
    #[validate(range(min = 1, max = 500))]
    pub capacity: u32,
    ...
}

fn validate_opening_hours(r: &RoomRequest) -> Result<(), ValidationError> {
    if r.opens_at >= r.closes_at {
        return Err(ValidationError::new("opening_hours").with_message("opensAt must be before closesAt".into()));
    }
    Ok(())
}

request.validate()?;   // Result<(), ValidationErrors> – all failures collected
```

Common rules: `length`, `range`, `email`, `url`, `regex(path = ...)`, `contains`, `must_match(other = ...)`,
`custom(function = ...)`, `nested` (validate inner structs), `schema` (struct-level, key `__all__`).

Alternative crates: `garde` (similar API, context support), `nutype` (validated newtypes via macro).

### Validating extractor

```rust
pub struct ValidatedJson<T>(pub T);

impl<S, T> FromRequest<S> for ValidatedJson<T>
where T: DeserializeOwned + Validate, S: Send + Sync
{
    type Rejection = Response;
    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(req, state).await.map_err(...)?;  // 400/415/422
        value.validate().map_err(...)?;                                              // 422 + fields
        Ok(Self(value))
    }
}

async fn create_room(ValidatedJson(request): ValidatedJson<RoomRequest>) -> Response { ... }
```

- Extractors are just types implementing `FromRequest` (body) or `FromRequestParts` (head only).
- A custom extractor centralizes cross-cutting input handling – handlers can't forget it.
- `Rejection` type controls the error response (here: JSON instead of plain text).

Error response:

```json
{
  "error": "validation failed",
  "fields": {
    "__all__": ["opensAt must be before closesAt"],
    "capacity": ["must be between 1 and 500"],
    "name": ["must have 1-100 characters"]
  }
}
```

### Parse, don't validate – newtypes

```rust
pub struct RoomName(String);                 // private field

impl RoomName {
    pub fn parse(value: &str) -> Result<Self, InvalidValue> { /* trim, non-empty, max length */ }
    pub fn as_str(&self) -> &str { &self.0 }
}

pub struct TimeRange { start: DateTime<Utc>, end: DateTime<Utc> }   // invariant: start < end
impl TimeRange {
    pub fn new(start, end) -> Result<Self, InvalidValue>;
    pub fn overlaps(&self, other: &TimeRange) -> bool;             // used in step 013
}
```

- The constructor is the only way in → every `RoomName` / `TimeRange` in the program is valid.
- Functions take `TimeRange` instead of two timestamps → no "forgot to check" bugs, self-documenting signatures.
- Normalization (trimming) happens once, at construction.
- `#[serde(try_from = "String")]` can make serde call the constructor directly – useful when a
  type is used in DTOs; here domain types stay serde-free.

### DTO → domain: `TryFrom`

```rust
impl TryFrom<RoomRequest> for NewRoom {
    type Error = InvalidValue;
    fn try_from(r: RoomRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            name: RoomName::parse(&r.name)?,
            opening_hours: OpeningHours::new(r.opens_at, r.closes_at)?,
            ...
        })
    }
}
```

## What changed in this branch

- `src/domain/validation.rs` (new) – `InvalidValue` error
- `src/domain/time_range.rs` (new) – `TimeRange` with `overlaps`, tests
- `src/domain/room.rs` – `RoomName`, `OpeningHours` newtypes, tests
- `src/domain/booking.rs` – `period: TimeRange` replaces `start_time`/`end_time`
- `src/api/extractors.rs` (new) – `ValidatedJson`, JSON error responses
- `src/api/dto/*` – `#[derive(Validate)]` rules, struct-level rules, `TryFrom` into domain
- `src/api/rooms.rs`, `src/api/bookings.rs` – `ValidatedJson`, `try_into()`
- repositories – adapted to the new domain types
- `Cargo.toml` – `validator`

## Try it

```bash
cargo test
cargo run
B=localhost:3000/api/v1
curl -X POST $B/rooms -H 'content-type: application/json' -d '{"name":"","capacity":0,"opensAt":"18:00","closesAt":"08:00"}'
curl -X POST $B/rooms -H 'content-type: application/json' -d '{"name":"   ","capacity":5}'           # domain: blank
curl -X POST $B/rooms -H 'content-type: application/json' -d '{"name":"  Yellow room ","capacity":5}' # trimmed
curl -X POST $B/rooms -d '{}'                                                                         # 415 as JSON
```

## Exercises

1. Write `ValidatedQuery<T>` for query strings and limit `minCapacity` to `1..=500`.
2. Limit booking duration to 8 hours – decide: DTO rule, `TimeRange` invariant, or business rule? Why?
3. Add `#[validate(email)]` to a new optional `contactEmail` field of the room.
