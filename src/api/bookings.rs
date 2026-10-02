use std::sync::Arc;

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
        authentication::AuthUser,
        authorization::AdminUser,
        dto::{
            bookings::{BookingQuery, BookingResponse, CreateBookingRequest},
            pagination::PageResponse,
        },
        error::ApiResult,
        extractors::{Json, Path, Query, ValidatedJson},
    },
    app::AppState,
    domain::booking_service::BookingService,
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

// `/bookings?roomId=...&status=ACTIVE` – all users' bookings: administrators only (step 021).
// Regular users use `/users/me/bookings`.
async fn list_bookings(
    _admin: AdminUser,
    State(bookings): State<Arc<BookingService>>,
    Query(query): Query<BookingQuery>,
) -> ApiResult<Json<PageResponse<BookingResponse>>> {
    let (filter, page) = query.into_domain()?;
    let bookings = bookings.list_bookings(&filter, page).await?;
    Ok(Json(bookings.into()))
}

// Owner or admin – decided by the domain, which knows the booking's owner.
async fn get_booking(
    State(bookings): State<Arc<BookingService>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<BookingResponse>> {
    let booking = bookings.get_booking(id, &(&user).into()).await?;
    Ok(Json(booking.into()))
}

// `AuthUser` (step 018) – only authenticated callers can book; they book for themselves.
// Extractor order: `AuthUser` reads headers, `ValidatedJson` consumes the body (last).
async fn create_booking(
    State(bookings): State<Arc<BookingService>>,
    user: AuthUser,
    ValidatedJson(request): ValidatedJson<CreateBookingRequest>,
) -> ApiResult<impl IntoResponse> {
    let booking = bookings
        .create_booking(request.into_new_booking(user.id)?)
        .await?;
    // Business metric (step 023) – technical metrics alone don't show how the product is used.
    metrics::counter!("bookings_created_total").increment(1);
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, format!("/api/v1/bookings/{}", booking.id))],
        Json(BookingResponse::from(booking)),
    ))
}

// Authentication (`AuthUser`) + authorization: only the owner or an admin (checked in the domain).
async fn cancel_booking(
    State(bookings): State<Arc<BookingService>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<BookingResponse>> {
    let booking = bookings.cancel_booking(id, &(&user).into()).await?;
    Ok(Json(booking.into()))
}
