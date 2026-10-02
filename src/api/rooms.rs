use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use uuid::Uuid;

use crate::{
    api::{
        dto::{
            bookings::BookingResponse,
            pagination::{PageQuery, PageResponse},
            rooms::{RoomQuery, RoomRequest, RoomResponse},
        },
        error::ApiResult,
        // Wrappers with Problem Details rejections (step 010) instead of `axum::extract::*`.
        extractors::{Json, Path, Query, ValidatedJson},
    },
    app::AppState,
    domain::{booking::BookingFilter, booking_service::BookingService, room_service::RoomService},
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

// `State<T>` extractor gives the handler access to the shared application state
// (the value passed to `Router::with_state`). Destructuring `State(service)` unwraps it.
// Since step 012 handlers take only the service they need – `State<Arc<RoomService>>` –
// extracted from `AppState` through `FromRef` (see `app.rs`).
//
// `Query<RoomQuery>` deserializes the query string: `/rooms?minCapacity=5&name=room`.
// Unknown parameters are ignored; a value of a wrong type (`minCapacity=abc`) -> 400.
//
// Mapping pattern used by every handler (step 008):
//   request DTO --into()--> domain type --service--> domain result --from()--> response DTO
//
// Error handling (step 010): handlers return `ApiResult<T>` = `Result<T, ApiError>`.
// `Result<T, E>` implements `IntoResponse` when both `T` and `E` do; `?` converts domain
// errors into `ApiError`, which renders Problem Details. No error responses built by hand.
async fn list_rooms(
    State(rooms): State<Arc<RoomService>>,
    Query(query): Query<RoomQuery>,
) -> ApiResult<Json<PageResponse<RoomResponse>>> {
    let (filter, page) = query.into_domain()?;
    let rooms = rooms.list_rooms(&filter, page).await?;
    // `Page<Room>` -> `PageResponse<RoomResponse>` through the generic `From` impl;
    // the target type is inferred from the function's return type.
    Ok(Json(rooms.into()))
}

// `Path<Uuid>` is an *extractor*: Axum parses the `{id}` segment into `Uuid` (any type
// implementing `Deserialize` works) before calling the handler. If parsing fails (e.g. `/rooms/abc`),
// the handler is not called at all and the rejection (400) is returned.
//
// Order of extractors: `State` and `Path` read only request *parts*, so any order works;
// a body extractor (`Json`) would have to be the last argument.
async fn get_room(
    State(rooms): State<Arc<RoomService>>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<RoomResponse>> {
    let room = rooms.get_room(id).await?;
    Ok(Json(room.into()))
}

// `Json<T>` as an argument deserializes the request body. It consumes the body,
// so it must be the LAST extractor. Rejections (handler not called):
// - missing/wrong `Content-Type` (must be `application/json`) -> 415 Unsupported Media Type
// - malformed JSON                                             -> 400 Bad Request
// - valid JSON, wrong shape (missing field, wrong type)        -> 422 Unprocessable Entity
//
// `ValidatedJson<T>` (step 009) = `Json<T>` + `T::validate()`; invalid input -> 422 with field errors.
async fn create_room(
    State(rooms): State<Arc<RoomService>>,
    ValidatedJson(request): ValidatedJson<RoomRequest>,
) -> ApiResult<impl IntoResponse> {
    // DTO -> domain command; domain invariants are checked here (`TryFrom`), `?` maps the error.
    let room = rooms.create_room(request.try_into()?).await?;

    // REST convention for creation: 201 Created + `Location` header pointing to the new resource.
    // Headers can be added as an array of (name, value) tuples in the response tuple.
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, format!("/api/v1/rooms/{}", room.id))],
        Json(RoomResponse::from(room)),
    ))
}

// PUT = full replacement of the resource (all fields required). PATCH would be a partial update.
async fn update_room(
    State(rooms): State<Arc<RoomService>>,
    Path(id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<RoomRequest>,
) -> ApiResult<Json<RoomResponse>> {
    let room = rooms.update_room(id, request.try_into()?).await?;
    Ok(Json(room.into()))
}

// 204 No Content – success without a response body.
async fn delete_room(
    State(rooms): State<Arc<RoomService>>,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    rooms.delete_room(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// Sub-resource: bookings belonging to one room. Reuses the booking service with a fixed filter.
// Several `State` extractors with different sub-states can be combined.
async fn list_room_bookings(
    State(rooms): State<Arc<RoomService>>,
    State(bookings): State<Arc<BookingService>>,
    Path(id): Path<Uuid>,
    Query(page): Query<PageQuery>,
) -> ApiResult<Json<PageResponse<BookingResponse>>> {
    // 404 for an unknown room instead of an empty list.
    rooms.get_room(id).await?;
    let filter = BookingFilter {
        room_id: Some(id),
        // Struct update syntax: remaining fields from `Default` (all `None`).
        ..Default::default()
    };
    let bookings = bookings.list_bookings(&filter, page.try_into()?).await?;
    Ok(Json(bookings.into()))
}
