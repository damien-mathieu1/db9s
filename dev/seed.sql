CREATE TABLE users (
    id serial PRIMARY KEY,
    name text NOT NULL,
    email text NOT NULL,
    active boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT now()
);

INSERT INTO users (name, email, active) VALUES
    ('Ada Lovelace', 'ada@example.com', true),
    ('Alan Turing', 'alan@example.com', true),
    ('Grace Hopper', 'grace@example.com', false);
