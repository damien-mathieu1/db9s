---
description: "Read-only connections in db9s: writes refused by the app and by the server."
---

# Read-only connections

Mark the connections you can't afford to touch, in the config or with the
read-only toggle of the connection form:

```toml
[[connections]]
name = "prod"
kind = "postgres"
url = "postgres://app:${PG_PASSWORD}@db.internal:5432/app"
read_only = true
```

Or open everything read-only for one session: `db9s --read-only`.

![read-only](/gif/read-only.gif)

A read-only connection shows `READ-ONLY` in the header, and writes are refused,
not just hidden:

| Backend | Raw queries allowed | Also enforced by |
|---|---|---|
| Postgres, MySQL, SQLite | one read statement (`SELECT`, `SHOW`, `EXPLAIN`...) | a read-only session on the server (`PRAGMA query_only` for SQLite), so `WITH x AS (DELETE ...) SELECT` fails too |
| MongoDB | find filters, pipelines without `$out` / `$merge` | |
| Redis | read commands only (`GET`, `HGETALL`, `SCAN`...) | |
| RabbitMQ | none, `/` publishes | |

Insert, edit, delete and purge are refused on all of them.

:::caution
This protects you from mistakes, not from someone set on writing (on Postgres,
`set_config()` can undo the session setting). For a hard guarantee, connect with
a database user that has no write access.
:::

Peeking at a RabbitMQ queue still requeues messages, which flags them as
redelivered.
