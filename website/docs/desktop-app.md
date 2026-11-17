---
sidebar_position: 5
description: "db9s desktop app: the same screens and keys in a native GPUI window, plus a grid, JSON inspector, tabs, query editor and live charts."
---

# Desktop app

`db9s-gui` is db9s in a native window, drawn with [GPUI](https://www.gpui.rs) (the
UI framework of the Zed editor). It reuses the TUI's core: **same connections,
same screens, same keys**. On top of that it does what a terminal can't.

```bash
brew install --cask damien-mathieu1/tap/db9s-gui   # macOS
cargo run --release -p db9s-gui                    # from source
```

It reads the same `config.toml` and history, and takes `--read-only` too.

## Grid

Rows show as a real grid: one line per row, one column per field, scrolling both
ways, only the visible rows laid out, so big pages stay smooth.

- Columns size themselves to their content; **drag a header's right edge** to
  resize (kept across runs).
- **Click a header** to sort: ascending, descending, natural order.
- Click a row to select it.
- `e` edits the row right in the grid: the current cell is highlighted, takes the
  cursor while you type, and the grid scrolls sideways to it; changed cells turn
  green.

## JSON inspector

With the rows focused, a panel under the grid shows the selected row as a tree:
objects and arrays start folded, click `▸` to unfold. Unfolded fields stay open
while you move through the table.

## Query editor

`/` opens a multi-line editor: `shift+enter` adds a line, SQL keywords, strings and
numbers are colored, and `tab` completes table names, column names and keywords.
`↑`/`↓` still walk the history.

## Command palette

`:` lists every match under the command bar instead of one greyed suggestion;
click one to run it.

## Tabs and split

| Shortcut | Action |
|---|---|
| `Cmd-T` | new tab, on the connections already open |
| `Cmd-W` (or `q` on connections) | close the tab |
| `Cmd-1`..`Cmd-9`, `Cmd-[` / `Cmd-]` | switch tab |
| `Cmd-\` | show a second tab side by side; click a pane to work in it |

## Live charts

RabbitMQ queues and Redis show sparklines under the grid, sampled every 2 seconds:
ready and unacked messages, publish rate and consumers for a queue; memory, ops/s,
keys and connected clients for Redis.

## Everything else

| Shortcut | Action |
|---|---|
| `Cmd-C` / `Cmd-Shift-C` | copy the selected row as JSON / the page as CSV |
| `Cmd +` / `Cmd -` / `Cmd 0` | zoom |
| `E` | edit the row in a GUI editor that waits: set `editor = "code --wait"` (or `zed --wait`) in the config; terminal editors like `vim` need the TUI |
| `Cmd-Q` | quit |

Column widths, window size, zoom and font are saved in `gui.json` next to
`config.toml`; set `"font": "JetBrains Mono"` there to change the font.
