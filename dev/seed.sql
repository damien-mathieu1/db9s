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

CREATE TABLE products (
    id serial PRIMARY KEY,
    name text NOT NULL,
    price numeric(10, 2) NOT NULL,
    in_stock boolean NOT NULL DEFAULT true,
    tags text[] NOT NULL DEFAULT '{}',
    attributes jsonb NOT NULL DEFAULT '{}'
);

INSERT INTO products (name, price, in_stock, tags, attributes) VALUES
    ('Keyboard', 49.90, true, '{hardware,input}', '{"layout": "azerty", "wireless": true}'),
    ('Monitor', 199.00, false, '{hardware,display}', '{"size": 27, "resolution": "2560x1440"}'),
    ('Mouse', 19.50, true, '{hardware,input}', '{"dpi": 1600}');
