# 006 – Routing and request handling

## Goal

Design a REST API for rooms and bookings: resource-oriented URLs, HTTP methods, status codes,
and Axum routing features (nesting, merging, fallbacks, extractors).

## Key concepts

### REST resources of the service

| Method | Path | Action | Success |
|--------|------|--------|---------|
| GET | `/api/v1/rooms?min_capacity=&name=` | list + filter | 200 |
| POST | `/api/v1/rooms` | create | 201 + `Location` |
| GET | `/api/v1/rooms/{id}` | read | 200 / 404 |
| PUT | `/api/v1/rooms/{id}` | replace | 200 / 404 |
| DELETE | `/api/v1/rooms/{id}` | delete | 204 / 404 |
| GET | `/api/v1/rooms/{id}/bookings` | sub-resource | 200 |
| GET | `/api/v1/bookings?room_id=&user_id=&status=` | list + filter | 200 |
| POST | `/api/v1/bookings` | create | 201 / 422 |
| GET | `/api/v1/bookings/{id}` | read | 200 / 404 |
| POST | `/api/v1/bookings/{id}/cancel` | state transition | 200 / 404 |

Conventions:
- plural nouns for collections, ids in the path, filters/paging in the query string,
- `GET` safe, `PUT`/`DELETE` idempotent, `POST` neither,
- `PUT` replaces the whole resource, `PATCH` changes some fields,
- actions that don't fit CRUD (cancel, approve) → `POST /resource/{id}/action` or `PATCH` of a status field,
- version in the path (`/api/v1`) – simple and visible; alternatives: header / media type.

### Router composition

```rust
Router::new()
    .route("/rooms", get(list).post(create))               // several methods on one path
    .route("/rooms/{id}", get(read).put(update).delete(remove))
    .route("/files/{*path}", get(serve_file))              // wildcard – rest of the path

Router::new()
    .merge(health::router())          // combine routers on the same level
    .nest("/api/v1", api_v1)          // mount under a prefix (prefix is stripped)
    .fallback(not_found)              // no route matched
    .method_not_allowed_fallback(h)   // path matched, method did not (Allow header is still set)
    .with_state(state)
```

- Each feature module exposes `fn router() -> Router<AppState>`; `api::router` assembles them.
- Route conflicts (same path + method registered twice) panic at startup – not at request time.
- `HEAD` is served automatically for `GET` routes.

### Extractors used so far

| Extractor | Example | Rejection |
|-----------|---------|-----------|
| `Path<u64>` | `/rooms/{id}` | 400 – cannot parse |
| `Path<(u64, u64)>` | `/rooms/{room_id}/bookings/{id}` – tuple in path order | 400 |
| `Path<Params>` | struct with fields named like the segments | 400 |
| `Query<Filter>` | `?min_capacity=5&name=blue` | 400 – wrong type |
| `Json<T>` | request body | 415 / 400 / 422 |
| `State<AppState>` | shared state | compile-time checked |
| `Uri`, `Method`, `HeaderMap` | raw request parts | never fails |
| `Option<Query<T>>`, `Result<Json<T>, JsonRejection>` | handle missing/invalid input yourself | – |

`Json<T>` rejections:

| Situation | Status |
|-----------|--------|
| no `Content-Type: application/json` | 415 Unsupported Media Type |
| syntactically invalid JSON | 400 Bad Request |
| valid JSON, wrong shape (missing field, wrong type) | 422 Unprocessable Entity |

The body extractor must be the **last** handler argument (it consumes the body stream).

### Responses with status and headers

```rust
(StatusCode::CREATED, [(header::LOCATION, "/api/v1/rooms/4")], Json(room))   // 201 + header + body
StatusCode::NO_CONTENT                                                        // 204, empty body
```

Different response types in branches → convert with `.into_response()` and return `Response`.

### Status codes used

| Code | Meaning in this API |
|------|---------------------|
| 200 OK | read / update / action succeeded |
| 201 Created | resource created, `Location` points to it |
| 204 No Content | deleted |
| 400 Bad Request | malformed input (path, query, JSON syntax) |
| 404 Not Found | resource / route does not exist |
| 405 Method Not Allowed | wrong method for an existing path |
| 415 Unsupported Media Type | body not JSON |
| 422 Unprocessable Entity | well-formed but semantically invalid (e.g. room does not exist) |

## What changed in this branch

- `src/api/mod.rs` – `/api/v1` nesting, JSON fallbacks for 404/405, URL map
- `src/api/rooms.rs` – full CRUD, `Query` filters, 201 + `Location`, 204, sub-resource
- `src/api/bookings.rs` (new) – list/create/read/cancel
- `src/domain/room.rs` – `NewRoom`, `RoomFilter`; `src/domain/booking.rs` (new) – `Booking`, `BookingStatus`, `NewBooking`, `BookingFilter`
- `src/domain/room_service.rs`, `src/domain/booking_service.rs` (new)
- `src/infrastructure/memory/*` – id generation with `AtomicU64`, insert/update/delete, booking repository
- `src/app.rs` – booking service wiring, shared room repository
- `Cargo.toml` – `chrono` (timestamps), `serde_json` (`json!`)

## Try it

```bash
cargo run
B=localhost:3000/api/v1
curl "$B/rooms?min_capacity=5"
curl -i -X POST $B/rooms -H 'content-type: application/json' -d '{"name":"Red room","capacity":6}'
curl -i -X POST $B/rooms -d '{"name":"x"}'                                          # 415
curl -i -X POST $B/rooms -H 'content-type: application/json' -d '{"name":"x"}'      # 422
curl -i -X POST $B/bookings -H 'content-type: application/json' \
  -d '{"room_id":1,"user_id":7,"start_time":"2026-10-05T09:00:00Z","end_time":"2026-10-05T10:00:00Z","attendees":4}'
curl -X POST $B/bookings/1/cancel
curl -i -X PATCH $B/rooms/1                                                         # 405 + Allow header
curl -i $B/unknown                                                                  # JSON 404
```

## Exercises

1. Add `GET /api/v1/users/{user_id}/bookings` using a `BookingFilter`.
2. Add `PATCH /api/v1/rooms/{id}` accepting `{"capacity": 10}` only (struct with `Option` fields).
3. Change `create_booking` to take `Result<Json<NewBooking>, JsonRejection>` and return a custom message for invalid bodies.
