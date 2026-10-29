-- Same data as dev/seed.sql, for MySQL.
CREATE TABLE users (
    id INT AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    email VARCHAR(100) NOT NULL,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO users (name, email, active) VALUES
    ('Ada Lovelace', 'ada@example.com', TRUE),
    ('Alan Turing', 'alan@example.com', TRUE),
    ('Grace Hopper', 'grace@example.com', FALSE);

CREATE TABLE products (
    id INT AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    price DECIMAL(10, 2) NOT NULL,
    in_stock BOOLEAN NOT NULL DEFAULT TRUE,
    attributes JSON NOT NULL
);

INSERT INTO products (name, price, in_stock, attributes) VALUES
    ('Keyboard', 49.90, TRUE, '{"layout": "azerty", "wireless": true}'),
    ('Monitor', 199.00, FALSE, '{"size": 27, "resolution": "2560x1440"}'),
    ('Mouse', 19.50, TRUE, '{"dpi": 1600}');
