---
description: "Run raw queries in db9s: SQL, Mongo filters and pipelines, Redis commands, RabbitMQ publish."
---

# Querying

`/` (with the rows focused) opens a prompt that runs a query in the backend's own
language, and shows the result in the rows pane.

![query](/gif/query.gif)

| Backend | `/` runs |
|---|---|
| Postgres, MySQL, SQLite | any SQL, in the schema being browsed (`select * from users` works without a prefix) |
| MongoDB | a find filter (`{"active": true}`), or an aggregation pipeline (`[{"$match": ...}, ...]`) |
| Redis | any command (`HGETALL user:1`, `SET greeting "hi"`), the raw reply shown like `redis-cli` |
| RabbitMQ | publishes the text as a message to the queue |

Statements that don't return rows (`UPDATE ...`) show how many rows they
affected.

## History

`↑` in the prompt brings back the previous query of this connection, `↓` the next
one. History survives restarts: it's kept in `history.json` next to
`config.toml`, 100 queries per connection, a repeated query moving to the end.

On a [read-only connection](../configuration/read-only.md), only queries that
can't write run.
