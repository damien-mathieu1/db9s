# db9s

A k9s-style terminal UI to browse, query, and edit Postgres and MongoDB from
one place — built with [ratatui](https://ratatui.rs).

## Install

```bash
git clone https://github.com/damien-mathieu1/db9s.git
cd db9s
cargo install --path .
```

## Usage

```bash
db9s --postgres postgres://user:pass@localhost/db
db9s --mongo mongodb://localhost:27017
```

Or configure persistent connections in
`~/.config/db9s/config.toml` (Linux) /
`~/Library/Application Support/db9s/config.toml` (macOS):

```toml
[[connections]]
name = "local pg"
kind = "postgres"
url = "postgres://user:pass@localhost/db"

[[connections]]
name = "local mongo"
kind = "mongo"
url = "mongodb://localhost:27017"

editor = "nvim"  # optional, used for row/doc edit — defaults to $EDITOR, then vi
```

You can also add, edit, delete connections and change settings from inside
the TUI (`a` / `e` / `d` / `s` on the connections screen).

Once inside a database, the browse screen is split-pane: containers on the
left, rows/documents (as cards) on the right.

## Keys

| Key | Action |
|---|---|
| `j`/`k` / arrows | move |
| `enter` | open connection / database / container |
| `tab` | switch focus between containers and rows pane |
| `esc` / `q` | back a level (`q` quits from connections) |
| `/` | raw query (SQL for postgres, JSON filter or `[$match, ...]` pipeline for mongo) |
| `e` | edit selected row/document inline, field by field (`enter` edit field, `ctrl+s` save) |
| `E` | edit selected row/document in `$EDITOR`, confirm diff to save |
| `d` | delete selected row/document |
| `n` / `p` | next / previous page |
| `r` | refresh |
| `a` / `e` / `d` / `s` | (connections screen) add / edit / delete a connection / settings |

## Notes

- Row edit/delete on Postgres assumes a primary key column named `id`.
- Extending to another backend: see [CONTRIBUTING.md](CONTRIBUTING.md).
- Color palette ported from [mongotui](https://github.com/kreulenk/mongotui).

## License

[MIT](LICENSE)
