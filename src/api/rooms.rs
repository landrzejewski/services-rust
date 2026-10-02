use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use uuid::Uuid;

use crate::{
    api::dto::{
        bookings::BookingResponse,
        rooms::{RoomQuery, RoomRequest, RoomResponse},
    },
    api::extractors::{ValidatedJson, invalid_value},
    app::AppState,
    domain::booking::BookingFilter,
};

pub fn router() -> Router<AppState> {
    Router::new()
        // Several methods on one path: chain `MethodRouter`s (`get(..).post(..)`).
        .route("/rooms", get(list_rooms).post(create_room))
        .route(
            "/rooms/{id}",
            get(get_room).put(update_room).delete(delete_room),
        )
        .route("/rooms/{id}/bookings", get(list_room_bookings))
}

// `State<AppState>` extractor gives the handler access to the shared application state
// (the value passed to `Router::with_state`). Destructuring `State(state)` unwraps it.
//
// `Query<RoomQuery>` deserializes the query string: `/rooms?minCapacity=5&name=room`.
// Unknown parameters are ignored; a value of a wrong type (`minCapacity=abc`) -> 400.
//
// Mapping pattern used by every handler (step 008):
//   request DTO --into()--> domain type --service--> domain result --from()--> response DTO
async fn list_rooms(
    State(state): State<AppState>,
    Query(query): Query<RoomQuery>,
) -> Json<Vec<RoomResponse>> {
    let rooms = state.room_service.list_rooms(&query.into()).await;
    // `into_iter().map(From::from).collect()` converts every element; the target type
    // `Vec<RoomResponse>` is inferred from the function's return type.
    Json(rooms.into_iter().map(RoomResponse::from).collect())
}

// `Path<Uuid>` is an *extractor*: Axum parses the `{id}` segment into `Uuid` (any type
// implementing `Deserialize` works) before calling the handler. If parsing fails (e.g. `/rooms/abc`), the handler
// is not called at all and Axum responds with `400 Bad Request`.
//
// Order of extractors: `State` and `Path` read only request *parts*, so any order works;
// a body extractor (`Json`) would have to be the last argument.
async fn get_room(State(state): State<AppState>, Path(id): Path<Uuid>) -> Response {
    // The handler only translates: domain `Option<Room>` -> HTTP 200 / 404.
    match state.room_service.get_room(id).await {
        Some(room) => Json(RoomResponse::from(room)).into_response(),
        None => room_not_found(id),
    }
}

// `Json<T>` as an argument deserializes the request body. It consumes the body,
// so it must be the LAST extractor. Rejections (handler not called):
// - missing/wrong `Content-Type` (must be `application/json`) -> 415 Unsupported Media Type
// - malformed JSON                                             -> 400 Bad Request
// - valid JSON, wrong shape (missing field, wrong type)        -> 422 Unprocessable Entity
//
// `ValidatedJson<T>` (step 009) = `Json<T>` + `T::validate()`; invalid input -> 422 with field errors.
async fn create_room(
    State(state): State<AppState>,
    ValidatedJson(request): ValidatedJson<RoomRequest>,
) -> Response {
    // DTO -> domain command; domain invariants are checked here (`TryFrom`).
    // Explicit `match` for now – step 010 shortens this to `request.try_into()?`.
    let new_room = match request.try_into() {
        Ok(new_room) => new_room,
        Err(error) => return invalid_value(error),
    };
    let room = state.room_service.create_room(new_room).await;

    // REST convention for creation: 201 Created + `Location` header pointing to the new resource.
    // Headers can be added as an array of (name, value) tuples in the response tuple.
    (
        StatusCode::CREATED,
        [(header::LOCATION, format!("/api/v1/rooms/{}", room.id))],
        Json(RoomResponse::from(room)),
    )
        .into_response()
}

// PUT = full replacement of the resource (all fields required). PATCH would be a partial update.
async fn update_room(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<RoomRequest>,
) -> Response {
    let data = match request.try_into() {
        Ok(data) => data,
        Err(error) => return invalid_value(error),
    };
    match state.room_service.update_room(id, data).await {
        Some(room) => Json(RoomResponse::from(room)).into_response(),
        None => room_not_found(id),
    }
}

// 204 No Content – success without a response body.
async fn delete_room(State(state): State<AppState>, Path(id): Path<Uuid>) -> Response {
    if state.room_service.delete_room(id).await {
        StatusCode::NO_CONTENT.into_response()
    } else {
        room_not_found(id)
    }
}

// Sub-resource: bookings belonging to one room. Reuses the booking service with a fixed filter.
async fn list_room_bookings(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<Vec<BookingResponse>> {
    let filter = BookingFilter {
        room_id: Some(id),
        // Struct update syntax: remaining fields from `Default` (all `None`).
        ..Default::default()
    };
    let bookings = state.booking_service.list_bookings(&filter).await;
    Json(bookings.into_iter().map(BookingResponse::from).collect())
}

fn room_not_found(id: Uuid) -> Response {
    (StatusCode::NOT_FOUND, format!("room {id} not found")).into_response()
}
