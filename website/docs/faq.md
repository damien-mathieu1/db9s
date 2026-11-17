---
sidebar_position: 7
description: "Frequent questions about db9s."
---

# FAQ

### Why can't I edit or delete rows of this table?

SQL edits find rows by primary key. A table without one is read-only. Also check
the header: a `READ-ONLY` connection refuses every write.

### Does peeking at a RabbitMQ queue consume the messages?

No, they're put back, but RabbitMQ flags them as redelivered.

### My query is slow, is db9s stuck?

No: every call runs in the background and the status line says what's loading.
`esc` cancels a read. Writes (saves, deletes, `/` queries) are never cancelled
halfway, so they always either apply or fail cleanly.

### `E` does nothing in the desktop app

The window has no terminal for `vim` or `nano`. Set a GUI editor that waits for
the file to close: `editor = "code --wait"` or `"zed --wait"`.

### macOS says db9s.app "can't be opened"

It isn't notarized yet. Run
`xattr -dr com.apple.quarantine /Applications/db9s.app` once.

### Where are my connections, history and settings?

See [Config file](configuration/config-file.md).

### My database isn't supported

SQL Server, ClickHouse and Cassandra are planned. A new SQL database is mostly a
`SqlDialect`; see
[CONTRIBUTING.md](https://github.com/damien-mathieu1/db9s/blob/main/CONTRIBUTING.md#adding-a-backend).
