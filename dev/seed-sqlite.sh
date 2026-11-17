#!/usr/bin/env bash
# SQLite has no server: this writes dev/db9s.sqlite with the same data as dev/seed.sql.
#   dev/seed-sqlite.sh && db9s --sqlite sqlite://$PWD/dev/db9s.sqlite
set -euo pipefail
db="$(dirname "$0")/db9s.sqlite"
rm -f "$db"
sqlite3 "$db" <<'SQL'
CREATE TABLE users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    email TEXT NOT NULL,
    active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
INSERT INTO users (name, email, active) VALUES
    ('Ada Lovelace', 'ada@example.com', 1),
    ('Alan Turing', 'alan@example.com', 1),
    ('Grace Hopper', 'grace@example.com', 0);

CREATE TABLE products (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    price REAL NOT NULL,
    in_stock INTEGER NOT NULL DEFAULT 1,
    attributes TEXT NOT NULL DEFAULT '{}'
);
INSERT INTO products (name, price, in_stock, attributes) VALUES
    ('Keyboard', 49.90, 1, '{"layout": "azerty", "wireless": true}'),
    ('Monitor', 199.00, 0, '{"size": 27, "resolution": "2560x1440"}'),
    ('Mouse', 19.50, 1, '{"dpi": 1600}');
SQL
echo "$db"
