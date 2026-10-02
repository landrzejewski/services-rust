use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};

use crate::{app::AppState, domain::room::Room};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/rooms", get(list_rooms))
        .route("/rooms/{id}", get(get_room))
}

// `State<AppState>` extractor gives the handler access to the shared application state
// (the value passed to `Router::with_state`). Destructuring `State(state)` unwraps it.
async fn list_rooms(State(state): State<AppState>) -> Json<Vec<Room>> {
    Json(state.room_service.list_rooms().await)
}

// `Path<u64>` is an *extractor*: Axum parses the `{id}` segment into `u64`
// before calling the handler. If parsing fails (e.g. `/rooms/abc`), the handler
// is not called at all and Axum responds with `400 Bad Request`.
//
// Order of extractors: `State` and `Path` read only request *parts*, so any order works;
// a body extractor (`Json`) would have to be the last argument.
async fn get_room(State(state): State<AppState>, Path(id): Path<u64>) -> Response {
    // The handler only translates: domain `Option<Room>` -> HTTP 200 / 404.
    match state.room_service.get_room(id).await {
        Some(room) => Json(room).into_response(),
        None => (StatusCode::NOT_FOUND, format!("room {id} not found")).into_response(),
    }
}
