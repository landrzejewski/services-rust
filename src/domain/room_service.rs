//! Business operations on rooms.

use std::sync::Arc;

use uuid::Uuid;

use crate::domain::{
    clock::Clock,
    error::{DomainError, DomainResult},
    pagination::{Page, PageRequest},
    repositories::{BookingRepository, RoomRepository},
    room::{NewRoom, Room, RoomFilter},
    transaction::TxManager,
};

pub struct RoomService {
    repository: Arc<dyn RoomRepository>,
    bookings: Arc<dyn BookingRepository>,
    tx_manager: Arc<dyn TxManager>,
    clock: Arc<dyn Clock>,
}

impl RoomService {
    // Constructor injection: dependencies are passed in, never created inside the service.
    pub fn new(
        repository: Arc<dyn RoomRepository>,
        bookings: Arc<dyn BookingRepository>,
        tx_manager: Arc<dyn TxManager>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            repository,
            bookings,
            tx_manager,
            clock,
        }
    }

    // `?` converts `RepositoryError` into `DomainError` (`From` impl in `domain::error`).
    pub async fn list_rooms(
        &self,
        filter: &RoomFilter,
        page: PageRequest,
    ) -> DomainResult<Page<Room>> {
        Ok(self.repository.find(filter, page).await?)
    }

    pub async fn get_room(&self, id: Uuid) -> DomainResult<Room> {
        // `ok_or_else` turns `Option<T>` into `Result<T, E>`; the closure builds the error
        // only when needed (`ok_or` would build it eagerly every time).
        self.repository
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::room_not_found(id))
    }

    pub async fn create_room(&self, new_room: NewRoom) -> DomainResult<Room> {
        Ok(self.repository.insert(new_room).await?)
    }

    pub async fn update_room(&self, id: Uuid, data: NewRoom) -> DomainResult<Room> {
        self.repository
            .update(id, data)
            .await?
            .ok_or_else(|| DomainError::room_not_found(id))
    }

    pub async fn delete_room(&self, id: Uuid) -> DomainResult<()> {
        // Check-then-delete in one transaction (step 016). Locking the room row is what
        // `BookingService::create_booking` does too, so a booking can't be created between
        // the count below and the delete.
        let mut tx = self.tx_manager.begin().await?;
        if self
            .repository
            .find_by_id_for_update(tx.as_mut(), id)
            .await?
            .is_none()
        {
            // Early return drops `tx` -> ROLLBACK.
            return Err(DomainError::room_not_found(id));
        }

        // Business rule: deleting a room would silently invalidate bookings users rely on.
        let upcoming = self
            .bookings
            .count_active_by_room(tx.as_mut(), id, self.clock.now())
            .await?;
        if upcoming > 0 {
            return Err(DomainError::Conflict(format!(
                "room {id} has {upcoming} upcoming booking(s); cancel them first"
            )));
        }

        if !self.repository.delete(tx.as_mut(), id).await? {
            return Err(DomainError::room_not_found(id));
        }
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use chrono::{NaiveTime, TimeDelta, Utc};

    use super::*;
    use crate::{
        domain::{
            booking::NewBooking,
            clock::SystemClock,
            repositories::{RepositoryError, RepositoryResult},
            room::{OpeningHours, RoomName},
            time_range::TimeRange,
            transaction::Transaction,
        },
        infrastructure::memory::{
            InMemoryBookingRepository, InMemoryRoomRepository, InMemoryTxManager,
        },
    };

    // Thanks to the trait, a test can inject any implementation – here a stub that always fails.
    // No database, no HTTP; tests the service's error handling in isolation.
    struct FailingRepository;

    fn failure() -> RepositoryError {
        RepositoryError::Unexpected {
            message: "connection lost".into(),
            source: None,
        }
    }

    #[async_trait]
    impl RoomRepository for FailingRepository {
        async fn find(&self, _: &RoomFilter, _: PageRequest) -> RepositoryResult<Page<Room>> {
            Err(failure())
        }
        async fn find_by_id(&self, _: Uuid) -> RepositoryResult<Option<Room>> {
            Err(failure())
        }
        async fn insert(&self, _: NewRoom) -> RepositoryResult<Room> {
            Err(failure())
        }
        async fn update(&self, _: Uuid, _: NewRoom) -> RepositoryResult<Option<Room>> {
            Err(failure())
        }
        async fn find_by_id_for_update(
            &self,
            _: &mut dyn Transaction,
            _: Uuid,
        ) -> RepositoryResult<Option<Room>> {
            Err(failure())
        }
        async fn delete(&self, _: &mut dyn Transaction, _: Uuid) -> RepositoryResult<bool> {
            Err(failure())
        }
    }

    #[tokio::test]
    async fn repository_failure_becomes_domain_error() {
        let service = RoomService::new(
            Arc::new(FailingRepository),
            Arc::new(InMemoryBookingRepository::new()),
            Arc::new(InMemoryTxManager::new()),
            Arc::new(SystemClock),
        );

        let result = service.get_room(Uuid::now_v7()).await;

        assert!(matches!(result, Err(DomainError::Repository(_))));
    }

    // Fixture: service over in-memory storage with one room.
    async fn setup() -> (
        RoomService,
        Arc<InMemoryBookingRepository>,
        Arc<InMemoryTxManager>,
        Uuid,
    ) {
        let rooms = Arc::new(InMemoryRoomRepository::new());
        let bookings = Arc::new(InMemoryBookingRepository::new());
        let tx_manager = Arc::new(InMemoryTxManager::new());
        let hour = |h| NaiveTime::from_hms_opt(h, 0, 0).unwrap();
        let room = rooms
            .insert(NewRoom {
                name: RoomName::parse("Test room").unwrap(),
                description: None,
                capacity: 4,
                opening_hours: OpeningHours::new(hour(0), hour(23)).unwrap(),
            })
            .await
            .unwrap();
        let service = RoomService::new(
            rooms,
            Arc::clone(&bookings) as Arc<dyn BookingRepository>,
            Arc::clone(&tx_manager) as Arc<dyn TxManager>,
            Arc::new(SystemClock),
        );
        (service, bookings, tx_manager, room.id)
    }

    #[tokio::test]
    async fn deletes_room_without_bookings() {
        let (service, _, _, room) = setup().await;

        service.delete_room(room).await.unwrap();

        assert!(matches!(
            service.get_room(room).await,
            Err(DomainError::NotFound { .. })
        ));
    }

    #[tokio::test]
    async fn refuses_to_delete_room_with_upcoming_booking() {
        let (service, bookings, tx_manager, room) = setup().await;
        let start = Utc::now() + TimeDelta::days(1);
        let mut tx = tx_manager.begin().await.unwrap();
        bookings
            .insert(
                tx.as_mut(),
                NewBooking {
                    room_id: room,
                    user_id: Uuid::now_v7(),
                    period: TimeRange::new(start, start + TimeDelta::hours(1)).unwrap(),
                    attendees: 1,
                },
            )
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let result = service.delete_room(room).await;

        assert!(matches!(result, Err(DomainError::Conflict(_))));
        assert!(service.get_room(room).await.is_ok());
    }

    #[tokio::test]
    async fn deleting_missing_room_is_not_found() {
        let (service, _, _, _) = setup().await;

        let result = service.delete_room(Uuid::now_v7()).await;

        assert!(matches!(result, Err(DomainError::NotFound { .. })));
    }
}
