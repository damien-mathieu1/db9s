---
description: "PostgreSQL, MySQL / MariaDB and SQLite in db9s."
---

# Postgres, MySQL and SQLite

All three share one engine, so they behave the same: databases are **schemas**,
containers are **tables**.

```bash
db9s --postgres postgres://user:pass@localhost:5432/db
db9s --mysql mysql://user:pass@localhost:3306/db
db9s --sqlite sqlite:///absolute/path/to/app.db
```

## SQLite

SQLite has no server: a database is a file and the URL is `sqlite://` followed by
its path. An absolute path starts with `/`, hence the three slashes:
`sqlite:///Users/me/app.db`. The file must exist; append `?mode=rwc` to create it.
The databases listed are `main` plus any `ATTACH`ed one.

## Queries

`/` runs any SQL in the schema being browsed, so `select * from users` needs no
schema prefix. Queries that return rows show them; others show the number of
rows affected.

## Editing

Updates and deletes find rows by the table's **primary key** (composite keys
work); a table without one is read-only. Values are always sent as bind
parameters.

## Read-only

On a [read-only connection](../configuration/read-only.md), `/` only runs a
single read statement (`SELECT`, `WITH ... SELECT`, `SHOW`, `EXPLAIN`...), and the
server enforces it too: a read-only session on Postgres and MySQL, `PRAGMA
query_only` on SQLite.
