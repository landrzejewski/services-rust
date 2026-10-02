use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use uuid::Uuid;

use crate::{
    app::AppState,
    domain::{
        booking::{Booking, BookingFilter},
        room::{NewRoom, Room, RoomFilter},
    },
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
// `Query<RoomFilter>` deserializes the query string: `/rooms?minCapacity=5&name=room`.
// Unknown parameters are ignored; a value of a wrong type (`minCapacity=abc`) -> 400.
async fn list_rooms(
    State(state): State<AppState>,
    Query(filter): Query<RoomFilter>,
) -> Json<Vec<Room>> {
    Json(state.room_service.list_rooms(&filter).await)
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
        Some(room) => Json(room).into_response(),
        None => room_not_found(id),
    }
}

// `Json<NewRoom>` as an argument deserializes the request body. It consumes the body,
// so it must be the LAST extractor. Rejections (handler not called):
// - missing/wrong `Content-Type` (must be `application/json`) -> 415 Unsupported Media Type
// - malformed JSON                                             -> 400 Bad Request
// - valid JSON, wrong shape (missing field, wrong type)        -> 422 Unprocessable Entity
async fn create_room(State(state): State<AppState>, Json(new_room): Json<NewRoom>) -> Response {
    let room = state.room_service.create_room(new_room).await;

    // REST convention for creation: 201 Created + `Location` header pointing to the new resource.
    // Headers can be added as an array of (name, value) tuples in the response tuple.
    (
        StatusCode::CREATED,
        [(header::LOCATION, format!("/api/v1/rooms/{}", room.id))],
        Json(room),
    )
        .into_response()
}

// PUT = full replacement of the resource (all fields required). PATCH would be a partial update.
async fn update_room(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(data): Json<NewRoom>,
) -> Response {
    match state.room_service.update_room(id, data).await {
        Some(room) => Json(room).into_response(),
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
) -> Json<Vec<Booking>> {
    let filter = BookingFilter {
        room_id: Some(id),
        // Struct update syntax: remaining fields from `Default` (all `None`).
        ..Default::default()
    };
    Json(state.booking_service.list_bookings(&filter).await)
}

fn room_not_found(id: Uuid) -> Response {
    (StatusCode::NOT_FOUND, format!("room {id} not found")).into_response()
}
