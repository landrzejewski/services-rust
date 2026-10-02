use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::{Validate, ValidationError};

use crate::{
    api::dto::pagination::page_request,
    domain::{
        booking::{Booking, BookingFilter, BookingStatus, NewBooking},
        pagination::PageRequest,
        time_range::TimeRange,
        validation::InvalidValue,
    },
};

/// Body of `POST /bookings`.
//
// `DateTime<Utc>` (de)serializes as an RFC 3339 string, e.g. "2026-10-05T09:00:00Z". On input any
// offset is accepted ("2026-10-05T11:00:00+02:00") and converted to UTC.
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[validate(schema(function = "validate_period", skip_on_field_errors = false))]
pub struct CreateBookingRequest {
    pub room_id: Uuid,
    // No `userId` since step 018: the owner is the authenticated caller. Taking it from the
    // body would let anyone book in someone else's name.
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    // `default` – absent field -> `u32::default()`... which is 0. Here we want 1, hence a function.
    #[serde(default = "one")]
    #[validate(range(min = 1, message = "must be at least 1"))]
    pub attendees: u32,
}

fn one() -> u32 {
    1
}

fn validate_period(request: &CreateBookingRequest) -> Result<(), ValidationError> {
    if request.start_time >= request.end_time {
        return Err(
            ValidationError::new("period").with_message("startTime must be before endTime".into())
        );
    }
    Ok(())
}

impl CreateBookingRequest {
    /// Request + authenticated user id -> domain command. (A plain `TryFrom` no longer fits:
    /// part of the data comes from outside the body.)
    pub fn into_new_booking(self, user_id: Uuid) -> Result<NewBooking, InvalidValue> {
        Ok(NewBooking {
            room_id: self.room_id,
            user_id,
            period: TimeRange::new(self.start_time, self.end_time)?,
            attendees: self.attendees,
        })
    }
}

/// API representation of the booking status.
///
/// A separate enum (instead of serializing `domain::BookingStatus`) keeps the wire values stable:
/// renaming or adding domain variants is a conscious API decision – the compiler forces the
/// mapping below to be updated (`match` must be exhaustive).
//
// Unit-only enums serialize as strings. `rename_all` changes the variant names:
// `Active` -> "ACTIVE", `Cancelled` -> "CANCELLED". Works in JSON bodies and query strings.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BookingStatusDto {
    Active,
    Cancelled,
}

impl From<BookingStatus> for BookingStatusDto {
    fn from(status: BookingStatus) -> Self {
        match status {
            BookingStatus::Active => Self::Active,
            BookingStatus::Cancelled => Self::Cancelled,
        }
    }
}

impl From<BookingStatusDto> for BookingStatus {
    fn from(status: BookingStatusDto) -> Self {
        match status {
            BookingStatusDto::Active => Self::Active,
            BookingStatusDto::Cancelled => Self::Cancelled,
        }
    }
}

/// Query string of `GET /bookings`: `?roomId=...&userId=...&status=ACTIVE`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookingQuery {
    pub room_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub status: Option<BookingStatusDto>,
    pub page: Option<u32>,
    pub size: Option<u32>,
}

impl BookingQuery {
    pub fn into_domain(self) -> Result<(BookingFilter, PageRequest), InvalidValue> {
        let page = page_request(self.page, self.size)?;
        let filter = BookingFilter {
            room_id: self.room_id,
            user_id: self.user_id,
            // `Option::map(Into::into)` converts the inner value when present.
            status: self.status.map(Into::into),
        };
        Ok((filter, page))
    }
}

/// Booking representation returned by the API.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookingResponse {
    pub id: Uuid,
    pub room_id: Uuid,
    pub user_id: Uuid,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    /// Derived field – exists only in the API representation.
    pub duration_minutes: i64,
    pub attendees: u32,
    pub status: BookingStatusDto,
    pub created_at: DateTime<Utc>,
}

impl From<Booking> for BookingResponse {
    fn from(booking: Booking) -> Self {
        Self {
            // Computed before the fields are moved out of `booking`.
            duration_minutes: booking.duration().num_minutes(),
            id: booking.id,
            room_id: booking.room_id,
            user_id: booking.user_id,
            start_time: booking.period.start(),
            end_time: booking.period.end(),
            attendees: booking.attendees,
            status: booking.status.into(),
            created_at: booking.created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn response_contains_derived_duration_and_api_status() {
        let start = Utc.with_ymd_and_hms(2026, 10, 5, 9, 0, 0).unwrap();
        let booking = Booking {
            id: Uuid::now_v7(),
            room_id: Uuid::now_v7(),
            user_id: Uuid::now_v7(),
            period: TimeRange::new(start, start + chrono::TimeDelta::minutes(90)).unwrap(),
            attendees: 3,
            status: BookingStatus::Active,
            created_at: start,
        };

        let json = serde_json::to_value(BookingResponse::from(booking)).unwrap();

        assert_eq!(json["durationMinutes"], 90);
        assert_eq!(json["status"], "ACTIVE");
        assert_eq!(json["startTime"], "2026-10-05T09:00:00Z");
    }
}
