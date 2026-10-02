-- Bookings of rooms.

CREATE TABLE bookings (
    id         UUID PRIMARY KEY,
    -- ON DELETE CASCADE: deleting a room removes its (past/cancelled) bookings.
    -- The domain only allows deleting rooms without upcoming bookings.
    -- Alternative keeping history: soft delete (`deleted_at` column) + RESTRICT.
    room_id    UUID NOT NULL REFERENCES rooms (id) ON DELETE CASCADE,
    user_id    UUID NOT NULL,
    start_time TIMESTAMPTZ NOT NULL,              -- TIMESTAMPTZ stores an instant (UTC internally)
    end_time   TIMESTAMPTZ NOT NULL,
    attendees  INTEGER NOT NULL CHECK (attendees > 0),
    -- TEXT + CHECK instead of a PostgreSQL ENUM type: adding values doesn't need ALTER TYPE.
    status     TEXT NOT NULL CHECK (status IN ('ACTIVE', 'CANCELLED')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT bookings_period_check CHECK (start_time < end_time)
);

-- Partial indexes: only active bookings take part in the rule queries.
CREATE INDEX bookings_room_period_idx ON bookings (room_id, start_time, end_time) WHERE status = 'ACTIVE';
CREATE INDEX bookings_user_active_idx ON bookings (user_id, end_time) WHERE status = 'ACTIVE';
