use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{
    booking::{Booking, BookingFilter, BookingStatus, NewBooking},
    pagination::{Page, PageRequest},
    repositories::{BookingRepository, RepositoryError, RepositoryResult},
    time_range::TimeRange,
};

/// `BookingRepository` backed by PostgreSQL.
pub struct PostgresBookingRepository {
    pool: PgPool,
}

impl PostgresBookingRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

struct BookingRow {
    id: Uuid,
    room_id: Uuid,
    user_id: Uuid,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    attendees: i32,
    status: String,
    created_at: DateTime<Utc>,
}

// Enum <-> TEXT column. Explicit functions keep the stored values independent of Rust names.
// (Alternative: `#[derive(sqlx::Type)] #[sqlx(type_name = "text", rename_all = "SCREAMING_SNAKE_CASE")]`
// on a dedicated infrastructure enum.)
fn status_to_db(status: BookingStatus) -> &'static str {
    match status {
        BookingStatus::Active => "ACTIVE",
        BookingStatus::Cancelled => "CANCELLED",
    }
}

fn status_from_db(value: &str) -> RepositoryResult<BookingStatus> {
    match value {
        "ACTIVE" => Ok(BookingStatus::Active),
        "CANCELLED" => Ok(BookingStatus::Cancelled),
        other => Err(RepositoryError::Unexpected {
            message: format!("unknown booking status in database: {other}"),
            source: None,
        }),
    }
}

impl TryFrom<BookingRow> for Booking {
    type Error = RepositoryError;

    fn try_from(row: BookingRow) -> Result<Self, Self::Error> {
        Ok(Booking {
            id: row.id,
            room_id: row.room_id,
            user_id: row.user_id,
            period: TimeRange::new(row.start_time, row.end_time)
                .map_err(|e| RepositoryError::unexpected("invalid booking row", e))?,
            attendees: u32::try_from(row.attendees)
                .map_err(|e| RepositoryError::unexpected("invalid booking attendees", e))?,
            status: status_from_db(&row.status)?,
            created_at: row.created_at,
        })
    }
}

fn to_domain(rows: Vec<BookingRow>) -> RepositoryResult<Vec<Booking>> {
    rows.into_iter().map(Booking::try_from).collect()
}

fn count_to_usize(count: i64) -> usize {
    usize::try_from(count).unwrap_or_default()
}

#[async_trait]
impl BookingRepository for PostgresBookingRepository {
    async fn find(
        &self,
        filter: &BookingFilter,
        page: PageRequest,
    ) -> RepositoryResult<Page<Booking>> {
        let status = filter.status.map(status_to_db);
        let rows = sqlx::query_as!(
            BookingRow,
            r#"
            SELECT id, room_id, user_id, start_time, end_time, attendees, status, created_at
            FROM bookings
            WHERE ($1::uuid IS NULL OR room_id = $1)
              AND ($2::uuid IS NULL OR user_id = $2)
              AND ($3::text IS NULL OR status = $3)
            ORDER BY start_time, id
            LIMIT $4 OFFSET $5
            "#,
            filter.room_id,
            filter.user_id,
            status,
            i64::from(page.size()),
            i64::try_from(page.offset()).unwrap_or(i64::MAX),
        )
        .fetch_all(&self.pool)
        .await?;

        let total = sqlx::query_scalar!(
            r#"
            SELECT count(*) AS "total!"
            FROM bookings
            WHERE ($1::uuid IS NULL OR room_id = $1)
              AND ($2::uuid IS NULL OR user_id = $2)
              AND ($3::text IS NULL OR status = $3)
            "#,
            filter.room_id,
            filter.user_id,
            status,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(Page {
            items: to_domain(rows)?,
            request: page,
            total: u64::try_from(total).unwrap_or_default(),
        })
    }

    async fn find_by_id(&self, id: Uuid) -> RepositoryResult<Option<Booking>> {
        let row = sqlx::query_as!(
            BookingRow,
            r#"
            SELECT id, room_id, user_id, start_time, end_time, attendees, status, created_at
            FROM bookings WHERE id = $1
            "#,
            id
        )
        .fetch_optional(&self.pool)
        .await?;
        row.map(Booking::try_from).transpose()
    }

    async fn insert(&self, new_booking: NewBooking) -> RepositoryResult<Booking> {
        let row = sqlx::query_as!(
            BookingRow,
            r#"
            INSERT INTO bookings (id, room_id, user_id, start_time, end_time, attendees, status)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING id, room_id, user_id, start_time, end_time, attendees, status, created_at
            "#,
            Uuid::now_v7(),
            new_booking.room_id,
            new_booking.user_id,
            new_booking.period.start(),
            new_booking.period.end(),
            i32::try_from(new_booking.attendees).unwrap_or(i32::MAX),
            status_to_db(BookingStatus::Active),
        )
        .fetch_one(&self.pool)
        .await?;
        row.try_into()
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: BookingStatus,
    ) -> RepositoryResult<Option<Booking>> {
        let row = sqlx::query_as!(
            BookingRow,
            r#"
            UPDATE bookings SET status = $2 WHERE id = $1
            RETURNING id, room_id, user_id, start_time, end_time, attendees, status, created_at
            "#,
            id,
            status_to_db(status),
        )
        .fetch_optional(&self.pool)
        .await?;
        row.map(Booking::try_from).transpose()
    }

    async fn find_active_overlapping(
        &self,
        room_id: Uuid,
        period: &TimeRange,
    ) -> RepositoryResult<Vec<Booking>> {
        // Half-open interval overlap: existing.start < new.end AND existing.end > new.start.
        // Served by the partial index `bookings_room_period_idx`.
        let rows = sqlx::query_as!(
            BookingRow,
            r#"
            SELECT id, room_id, user_id, start_time, end_time, attendees, status, created_at
            FROM bookings
            WHERE room_id = $1 AND status = 'ACTIVE'
              AND start_time < $3 AND end_time > $2
            ORDER BY start_time
            "#,
            room_id,
            period.start(),
            period.end(),
        )
        .fetch_all(&self.pool)
        .await?;
        to_domain(rows)
    }

    async fn count_active_by_user(
        &self,
        user_id: Uuid,
        from: DateTime<Utc>,
    ) -> RepositoryResult<usize> {
        let count = sqlx::query_scalar!(
            r#"
            SELECT count(*) AS "count!" FROM bookings
            WHERE user_id = $1 AND status = 'ACTIVE' AND end_time > $2
            "#,
            user_id,
            from,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(count_to_usize(count))
    }

    async fn count_active_by_room(
        &self,
        room_id: Uuid,
        from: DateTime<Utc>,
    ) -> RepositoryResult<usize> {
        let count = sqlx::query_scalar!(
            r#"
            SELECT count(*) AS "count!" FROM bookings
            WHERE room_id = $1 AND status = 'ACTIVE' AND end_time > $2
            "#,
            room_id,
            from,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(count_to_usize(count))
    }
}
