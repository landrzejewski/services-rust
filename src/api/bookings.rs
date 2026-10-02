use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};

use crate::{
    app::AppState,
    domain::booking::{Booking, BookingFilter, NewBooking},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/bookings", get(list_bookings).post(create_booking))
        .route("/bookings/{id}", get(get_booking))
        // Cancelling is a state transition, not a deletion – modelled as an action endpoint.
        // Alternatives: `PATCH /bookings/{id}` with `{"status": "Cancelled"}`, or `DELETE`
        // when the record may disappear.
        .route("/bookings/{id}/cancel", post(cancel_booking))
}

// `/bookings?room_id=1&status=Active`
async fn list_bookings(
    State(state): State<AppState>,
    Query(filter): Query<BookingFilter>,
) -> Json<Vec<Booking>> {
    Json(state.booking_service.list_bookings(&filter).await)
}

async fn get_booking(State(state): State<AppState>, Path(id): Path<u64>) -> Response {
    match state.booking_service.get_booking(id).await {
        Some(booking) => Json(booking).into_response(),
        None => booking_not_found(id),
    }
}

async fn create_booking(
    State(state): State<AppState>,
    Json(new_booking): Json<NewBooking>,
) -> Response {
    let room_id = new_booking.room_id;
    match state.booking_service.create_booking(new_booking).await {
        Some(booking) => (
            StatusCode::CREATED,
            [(header::LOCATION, format!("/api/v1/bookings/{}", booking.id))],
            Json(booking),
        )
            .into_response(),
        // The referenced room does not exist – the request itself is wrong, not the URL.
        None => (
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("room {room_id} does not exist"),
        )
            .into_response(),
    }
}

async fn cancel_booking(State(state): State<AppState>, Path(id): Path<u64>) -> Response {
    match state.booking_service.cancel_booking(id).await {
        Some(booking) => Json(booking).into_response(),
        None => booking_not_found(id),
    }
}

fn booking_not_found(id: u64) -> Response {
    (StatusCode::NOT_FOUND, format!("booking {id} not found")).into_response()
}
