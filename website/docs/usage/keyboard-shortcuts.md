---
description: "Every db9s key, by screen."
---

# Keyboard shortcuts

The footer always shows the keys available where you are.

## Everywhere

| Key | Action |
|---|---|
| `j` / `k`, arrows | move |
| `enter` | open |
| `esc` | cancel a loading read, else back one level |
| `q` | back (quits from the connections screen) |
| `:` | [command bar](command-bar.md) |
| `ctrl+c` | quit |

## Connections screen

| Key | Action |
|---|---|
| `a` / `e` / `d` | add / edit / delete a connection (`ctrl+t` in the form tests it) |
| `s` | settings: editor and theme |

## Tables and rows

| Key | Action |
|---|---|
| `tab` | switch between the containers and the rows |
| `/` | [raw query](querying.md); `↑`/`↓` walk past queries |
| `e` | [edit](editing.md) the selected row field by field, `ctrl+s` saves |
| `E` | edit it in `$EDITOR`, confirm the diff to save |
| `a` | insert a row |
| `d` | delete the row (purge the queue for RabbitMQ) |
| `i` | [schema](schema.md) of the table or collection |
| `n` / `p` | next / previous page |
| `r` | refresh |

## In text fields

`←`/`→` move the cursor, `home`/`end` (or `ctrl+a`/`ctrl+e`) jump, `delete`
removes forward, `ctrl+u` clears before the cursor.

The [desktop app](../desktop-app.md) adds mouse support and `Cmd` shortcuts.
