---
description: "Redis in db9s: logical databases, keys by type, raw commands and live metrics."
---

# Redis

```bash
db9s --redis redis://localhost:6379      # rediss:// for TLS
```

Databases are the logical DBs (`db0` plus every non-empty one), containers are
**keys** (the first 1000 from `SCAN`), and rows follow the key type:

![redis](/gif/redis.gif)

| Type | Rows | Edit (`e`) | Delete (`d`) |
|---|---|---|---|
| string | `value` | `SET` (keeps the TTL) | deletes the key |
| hash | `field`, `value` | change value or rename field | `HDEL` |
| list | `index`, `value` | `LSET` | removes that index |
| set | `member` | rename member | `SREM` |
| zset | `member`, `score` | change score or member | `ZREM` |
| stream | `id`, `fields` | read-only | `XDEL` |

`/` runs any Redis command (`HGETALL user:1`, `SET greeting "hi there"`) and shows
the raw reply, like `redis-cli`. On a read-only connection only read commands run.

The [desktop app](../desktop-app.md) charts memory, ops/s, keys and clients live.
