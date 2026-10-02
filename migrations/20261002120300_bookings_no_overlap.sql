-- Database-level guarantee that active bookings of one room never overlap (step 016).
--
-- An EXCLUDE constraint generalizes UNIQUE: no two rows may satisfy all listed operators at once.
--   room_id WITH =                                   same room
--   tstzrange(start_time, end_time, '[)') WITH &&    overlapping half-open time ranges
--   WHERE (status = 'ACTIVE')                        only active bookings count
-- GiST indexes support `&&` on ranges; `btree_gist` adds GiST support for `=` on UUID.
-- Violations raise SQLSTATE 23P01 (exclusion_violation).

CREATE EXTENSION IF NOT EXISTS btree_gist;

ALTER TABLE bookings
    ADD CONSTRAINT bookings_no_overlap
    EXCLUDE USING gist (
        room_id WITH =,
        tstzrange(start_time, end_time, '[)') WITH &&
    ) WHERE (status = 'ACTIVE');
