# 007 – Serialization and deserialization

## Goal

Control the JSON format of the API with serde: naming, optional and default values, enums,
dates/times, identifiers and custom formats.

## Key concepts

### serde in one paragraph

`serde` defines two traits, `Serialize` and `Deserialize`, usually derived. Format crates
(`serde_json`, `toml`, `serde_urlencoded` used by `Query`, ...) implement the data format.
The same derive works for JSON bodies, query strings, config files.

```rust
let json: String = serde_json::to_string(&room)?;
let room: NewRoom = serde_json::from_str(&json)?;
let value = serde_json::json!({ "error": "not found" });   // ad-hoc JSON
```

### Container attributes (`#[serde(...)]` on struct / enum)

| Attribute | Effect |
|-----------|--------|
| `rename_all = "camelCase"` | `opens_at` ↔ `opensAt` (also `snake_case`, `kebab-case`, `SCREAMING_SNAKE_CASE`, `lowercase`...) |
| `deny_unknown_fields` | unknown input field → error (catches typos; limits forward compatibility) |
| `default` | missing fields taken from the struct's `Default` impl |
| `tag = "type"` | internally tagged enum: `{"type": "Email", "address": "..."}` |
| `tag = "t", content = "c"` | adjacently tagged: `{"t": "Email", "c": {...}}` |
| `untagged` | try variants in order, no tag in JSON |
| `transparent` | newtype serialized as its inner value (step 009) |

### Field attributes

| Attribute | Effect |
|-----------|--------|
| `rename = "roomName"` | name of one field |
| `alias = "room_name"` | additionally accepted input name |
| `default` / `default = "path::fn"` | value when absent |
| `skip` | never (de)serialized (must have `Default` for deserialization) |
| `skip_serializing_if = "Option::is_none"` | omit from output when the predicate is true |
| `with = "module"` | custom (de)serialization (`serialize_with` / `deserialize_with` for one direction) |
| `flatten` | inline fields of a nested struct |

### Option, null and absent fields

| JSON input | `Option<T>` field | `Option<T>` + `default` |
|------------|-------------------|-------------------------|
| field absent | `None` | `None` |
| `null` | `None` | `None` |
| value | `Some(v)` | `Some(v)` |

To distinguish "absent" from `null` (PATCH semantics) use `Option<Option<T>>` with
`#[serde(default, with = "::serde_with::rust::double_option")]` or a custom enum.

### Enums

```rust
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum BookingStatus { Active, Cancelled }        // "ACTIVE", "CANCELLED"
```

Unknown values are rejected: `?status=active` → `400 unknown variant`.

### Identifiers – UUID

```toml
uuid = { version = "1", features = ["v7", "serde"] }
```

- serialized as string `"0199a3f0-6f32-7608-b5f4-72e548b95c96"`, `Path<Uuid>` works directly,
- **v7**: timestamp prefix + random → sortable by creation time, good for DB indexes (vs fully random **v4**),
- not enumerable like `1, 2, 3` – doesn't leak counts, harder to guess (still not an authorization mechanism).

### Dates and times – chrono

| Type | Meaning | JSON (default) |
|------|---------|----------------|
| `DateTime<Utc>` | instant in UTC | `"2026-10-05T09:00:00Z"` (RFC 3339) |
| `DateTime<FixedOffset>` | instant + offset | `"2026-10-05T11:00:00+02:00"` |
| `NaiveDate` | calendar date | `"2026-10-05"` |
| `NaiveTime` | time of day | `"08:30:00"` (custom `"08:30"` here) |
| `NaiveDateTime` | date+time without zone | avoid for instants |

- Input `"2026-10-05T11:00:00+02:00"` into `DateTime<Utc>` → stored as `09:00:00Z`.
- Unix timestamps: `#[serde(with = "chrono::serde::ts_seconds")]`.
- Alternative crate: `time` (also supported by sqlx); `jiff` (newer, time-zone aware).

Rule: store and compute instants in UTC; convert to a time zone only for presentation.

### Custom format with `with`

```rust
#[serde(with = "hh_mm")]
pub opens_at: NaiveTime,

pub mod hh_mm {
    pub fn serialize<S: Serializer>(t: &NaiveTime, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&t.format("%H:%M"))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<NaiveTime, D::Error> {
        let text = String::deserialize(d)?;
        NaiveTime::parse_from_str(&text, "%H:%M").map_err(D::Error::custom)
    }
}
```

Errors created with `D::Error::custom` appear in the standard rejection, including the field path:
`closesAt: invalid time "8pm", expected HH:MM`.

## What changed in this branch

- `src/serde_formats.rs` (new) – `hh_mm` custom format + unit tests
- `src/domain/room.rs` – `Uuid` id, `description` (optional, skipped when `None`), opening hours,
  `camelCase`, `deny_unknown_fields`, defaults
- `src/domain/booking.rs` – `Uuid` ids, `createdAt`, `SCREAMING_SNAKE_CASE` status, default `attendees`
- repositories, services, handlers – `u64` → `Uuid` (`Uuid::now_v7()` replaces `AtomicU64` counters)
- `Cargo.toml` – `uuid` (features `v7`, `serde`)

API format change: JSON fields and query parameters are now camelCase (`roomId`, `minCapacity`),
status values uppercase (`ACTIVE`).

## Try it

```bash
cargo test
cargo run
B=localhost:3000/api/v1
curl $B/rooms
ROOM=$(curl -s "$B/rooms?name=blue" | jq -r '.[0].id')
curl -X POST $B/rooms -H 'content-type: application/json' -d '{"name":"Red room","capacity":6,"opensAt":"09:30"}'
curl -X POST $B/rooms -H 'content-type: application/json' -d '{"name":"Red","capasity":6}'          # unknown field
curl -X POST $B/rooms -H 'content-type: application/json' -d '{"name":"Red","capacity":6,"closesAt":"8pm"}'
curl -X POST $B/bookings -H 'content-type: application/json' \
  -d "{\"roomId\":\"$ROOM\",\"userId\":\"0199a3f0-0000-7000-8000-000000000007\",\"startTime\":\"2026-10-05T11:00:00+02:00\",\"endTime\":\"2026-10-05T12:00:00+02:00\"}"
curl "$B/bookings?status=ACTIVE"
curl "$B/bookings?status=active"     # 400 unknown variant
```

## Exercises

1. Add a `tags: Vec<String>` field to rooms that defaults to an empty list and is omitted from output when empty.
2. Add `#[serde(alias = "maxPeople")]` to `capacity` and check both names are accepted.
3. Serialize `createdAt` as Unix seconds using `chrono::serde::ts_seconds`.
