---
description: "How db9s screens are organized: connections, databases, containers and rows."
---

# Navigation

db9s has the same three levels for every backend:

| Level | Postgres / MySQL / SQLite | MongoDB | Redis | RabbitMQ |
|---|---|---|---|---|
| Database | schema | database | logical db (`db0`...) | vhost |
| Container | table | collection | key | queue |
| Row | row | document | entry (field, member, item...) | message |

![navigation](/gif/navigation.gif)

- **Connections**: `enter` opens one. A connection that failed shows its error in
  the list; the others still work.
- **Databases**: `enter` lists its containers.
- **Browse**: containers on the left, rows on the right. Moving through the
  containers previews their rows; `enter` or `tab` moves to the rows, `tab` again
  back to the containers.
- `esc` goes back one level, `q` too (and quits from the connections screen).

Every call to the server runs in the background: the screen never freezes on a
slow server, the status line says what's loading, and `esc` cancels a read that
takes too long. Writes are never cancelled halfway.
