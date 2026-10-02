-- Sample data for development and the training exercises.
-- Fixed ids make the requests in `requests/*.http` reproducible.
-- (In real projects seed data usually lives outside schema migrations, e.g. a separate script.)

INSERT INTO rooms (id, name, description, capacity, opens_at, closes_at) VALUES
    ('0199a3f0-0000-7000-8000-000000000001', 'Blue room', 'Projector, whiteboard', 8, '08:00', '18:00'),
    ('0199a3f0-0000-7000-8000-000000000002', 'Green room', NULL, 4, '08:00', '18:00'),
    ('0199a3f0-0000-7000-8000-000000000003', 'Conference hall', 'Stage, 2 projectors, sound system', 40, '07:00', '22:00');
