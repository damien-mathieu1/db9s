---
description: "Where db9s keeps its config and what goes in it."
---

# Config file

Connections added in the app are saved to:

- Linux: `~/.config/db9s/config.toml`
- macOS: `~/Library/Application Support/db9s/config.toml`

```toml
editor = "nvim"   # for `E`; defaults to $EDITOR, then vi
theme = "nord"    # see Themes

[[connections]]
name = "local pg"
kind = "postgres"           # postgres | mysql | sqlite | mongo | redis | rabbitmq
url = "postgres://user:pass@localhost/db"

[[connections]]
name = "app db"
kind = "sqlite"
url = "sqlite:///Users/me/app.db"

[[connections]]
name = "prod"
kind = "postgres"
url = "postgres://app:${PG_PASSWORD}@db.internal:5432/app"
read_only = true
```

Command-line flags (`--postgres URL`, `--sqlite URL`...) add a one-off connection
for the session without touching the file.

Next to it, db9s keeps:

| File | What |
|---|---|
| `history.json` | query history, per connection |
| `gui.json` | the desktop app's column widths, window size, zoom and font |
| `themes/<name>.toml` | your own [themes](themes.md) |
