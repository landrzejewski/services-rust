use axum::{
    Router,
    extract::State,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use uuid::Uuid;

use crate::{
    api::{
        dto::bookings::{BookingQuery, BookingResponse, CreateBookingRequest},
        error::ApiResult,
        extractors::{Json, Path, Query, ValidatedJson},
    },
    app::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/bookings", get(list_bookings).post(create_booking))
        .route("/bookings/{id}", get(get_booking))
        // Cancelling is a state transition, not a deletion – modelled as an action endpoint.
        // Alternatives: `PATCH /bookings/{id}` with `{"status": "CANCELLED"}`, or `DELETE`
        // when the record may disappear.
        .route("/bookings/{id}/cancel", post(cancel_booking))
}

// `/bookings?roomId=...&status=ACTIVE`
async fn list_bookings(
    State(state): State<AppState>,
    Query(query): Query<BookingQuery>,
) -> ApiResult<Json<Vec<BookingResponse>>> {
    let bookings = state.booking_service.list_bookings(&query.into()).await?;
    Ok(Json(
        bookings.into_iter().map(BookingResponse::from).collect(),
    ))
}

async fn get_booking(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<BookingResponse>> {
    let booking = state.booking_service.get_booking(id).await?;
    Ok(Json(booking.into()))
}

async fn create_booking(
    State(state): State<AppState>,
    ValidatedJson(request): ValidatedJson<CreateBookingRequest>,
) -> ApiResult<impl IntoResponse> {
    let booking = state
        .booking_service
        .create_booking(request.try_into()?)
        .await?;
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, format!("/api/v1/bookings/{}", booking.id))],
        Json(BookingResponse::from(booking)),
    ))
}

async fn cancel_booking(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<BookingResponse>> {
    let booking = state.booking_service.cancel_booking(id).await?;
    Ok(Json(booking.into()))
}
