//! Registration, login and "current user" endpoints (step 018).

use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};

use crate::{
    api::{
        authentication::AuthUser,
        dto::{
            bookings::BookingResponse,
            pagination::{PageQuery, PageResponse},
            users::{LoginRequest, RegisterRequest, UserResponse},
        },
        error::ApiResult,
        extractors::{Json, Query, ValidatedJson},
    },
    app::AppState,
    domain::{auth_service::AuthService, booking::BookingFilter, booking_service::BookingService},
};

pub fn router() -> Router<AppState> {
    Router::new()
        // Public endpoints.
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        // Protected endpoints – protected simply by taking `AuthUser` as an argument.
        .route("/users/me", get(me))
        .route("/users/me/bookings", get(my_bookings))
}

async fn register(
    State(auth): State<Arc<AuthService>>,
    ValidatedJson(request): ValidatedJson<RegisterRequest>,
) -> ApiResult<impl IntoResponse> {
    let (email, password) = request.into_domain()?;
    let user = auth.register(email, password).await?;
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, "/api/v1/users/me".to_string())],
        Json(UserResponse::from(user)),
    ))
}

// Step 018: login only verifies credentials and returns the user.
// Step 019 turns it into the endpoint issuing access tokens.
async fn login(
    State(auth): State<Arc<AuthService>>,
    Json(request): Json<LoginRequest>,
) -> ApiResult<Json<UserResponse>> {
    let user = auth.authenticate(&request.email, &request.password).await?;
    Ok(Json(user.into()))
}

async fn me(State(auth): State<Arc<AuthService>>, user: AuthUser) -> ApiResult<Json<UserResponse>> {
    Ok(Json(auth.get_user(user.id).await?.into()))
}

async fn my_bookings(
    State(bookings): State<Arc<BookingService>>,
    user: AuthUser,
    Query(page): Query<PageQuery>,
) -> ApiResult<Json<PageResponse<BookingResponse>>> {
    let filter = BookingFilter {
        user_id: Some(user.id),
        ..Default::default()
    };
    Ok(Json(
        bookings
            .list_bookings(&filter, page.try_into()?)
            .await?
            .into(),
    ))
}
