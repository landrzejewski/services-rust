use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use uuid::Uuid;

use crate::{
    api::{
        dto::bookings::{BookingQuery, BookingResponse, CreateBookingRequest},
        extractors::{ValidatedJson, invalid_value},
    },
    app::AppState,
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

// `/bookings?roomId=...&status=ACTIVE`
async fn list_bookings(
    State(state): State<AppState>,
    Query(query): Query<BookingQuery>,
) -> Json<Vec<BookingResponse>> {
    let bookings = state.booking_service.list_bookings(&query.into()).await;
    Json(bookings.into_iter().map(BookingResponse::from).collect())
}

async fn get_booking(State(state): State<AppState>, Path(id): Path<Uuid>) -> Response {
    match state.booking_service.get_booking(id).await {
        Some(booking) => Json(BookingResponse::from(booking)).into_response(),
        None => booking_not_found(id),
    }
}

async fn create_booking(
    State(state): State<AppState>,
    ValidatedJson(request): ValidatedJson<CreateBookingRequest>,
) -> Response {
    let room_id = request.room_id;
    let new_booking = match request.try_into() {
        Ok(new_booking) => new_booking,
        Err(error) => return invalid_value(error),
    };
    match state.booking_service.create_booking(new_booking).await {
        Some(booking) => (
            StatusCode::CREATED,
            [(header::LOCATION, format!("/api/v1/bookings/{}", booking.id))],
            Json(BookingResponse::from(booking)),
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

async fn cancel_booking(State(state): State<AppState>, Path(id): Path<Uuid>) -> Response {
    match state.booking_service.cancel_booking(id).await {
        Some(booking) => Json(BookingResponse::from(booking)).into_response(),
        None => booking_not_found(id),
    }
}

fn booking_not_found(id: Uuid) -> Response {
    (StatusCode::NOT_FOUND, format!("booking {id} not found")).into_response()
}
