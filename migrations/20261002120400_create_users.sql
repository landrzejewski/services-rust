-- Users and their roles (step 018).

CREATE TABLE users (
    id            UUID PRIMARY KEY,
    email         TEXT NOT NULL,
    -- Argon2id hash in PHC string format ($argon2id$v=19$m=...,t=...,p=...$salt$hash).
    -- NEVER the password itself. NULL for users authenticated by an external identity
    -- provider (step 020) – they have no local password.
    password_hash TEXT,
    role          TEXT NOT NULL DEFAULT 'USER' CHECK (role IN ('USER', 'ADMIN')),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Emails are compared case-insensitively.
CREATE UNIQUE INDEX users_email_idx ON users (lower(email));

-- Development accounts (passwords: admin-password-123 / user-password-123).
-- Real systems create the first admin through a bootstrap procedure, not a migration.
INSERT INTO users (id, email, password_hash, role) VALUES
    ('0199a3f0-0000-7000-8000-0000000000a1', 'admin@booking.local',
     '$argon2id$v=19$m=19456,t=2,p=1$jSifL9dWJLc5N0qIdBKSOQ$596DVVpoVRU7Cvhq+okHAkcHJ7ajTy9HUwpQ7EW5V6Q', 'ADMIN'),
    ('0199a3f0-0000-7000-8000-0000000000a2', 'user@booking.local',
     '$argon2id$v=19$m=19456,t=2,p=1$sz8i7xww0v0Rm2jWcsA8Dw$HbpKKThviUkGrSYeXph9iZktzfWFj0jCo0PHquc4RHM', 'USER');

-- Bookings now reference real users. Development bookings of unknown users are removed first,
-- otherwise the foreign key could not be created.
DELETE FROM bookings WHERE user_id NOT IN (SELECT id FROM users);
ALTER TABLE bookings
    ADD CONSTRAINT bookings_user_fk FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE;
