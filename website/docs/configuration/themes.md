---
description: "db9s themes: six built-ins and your own in TOML."
---

# Themes

Pick one in settings (`s` on the connections screen, `tab`, then `←`/`→` for a live
preview) or with `:theme <name>`. The choice is saved to the config
(`theme = "nord"`), and the desktop app uses it too.

| | |
|---|---|
| `default` ![default](/img/themes/default.png) | `catppuccin-mocha` ![catppuccin-mocha](/img/themes/catppuccin-mocha.png) |
| `dracula` ![dracula](/img/themes/dracula.png) | `gruvbox` ![gruvbox](/img/themes/gruvbox.png) |
| `nord` ![nord](/img/themes/nord.png) | `tokyo-night` ![tokyo-night](/img/themes/tokyo-night.png) |

## Your own

Create `themes/<name>.toml` next to `config.toml`. Start from a built-in and only
override what changes; a file named like a built-in replaces it. `:theme <name>`
re-reads the file, so you can edit and reload as you go.

```toml
inherits = "nord"          # optional, defaults to "default"
border_type = "rounded"    # plain | rounded | double | thick

[colors]                   # "#rrggbb", an ANSI index like "67", or a name like "red"
border_focus = "#ff5f87"   # focused pane border, header title
border_blur  = "#4c566a"   # unfocused borders
selected_fg  = "#2e3440"   # selected row / field text
selected_bg  = "#88c0d0"   # selected row / field background
field_name   = "#a3be8c"   # field names in rows and editors
modal_border = "#b48ead"   # popups
error        = "#bf616a"
success      = "#a3be8c"
key_hint     = "#ebcb8b"   # keys in the footer
muted        = "#616e88"   # descriptions, urls, hints
```

A typo in a key or an invalid color shows an error and keeps the current theme.
The built-ins are in
[`crates/db9s-core/src/themes/`](https://github.com/damien-mathieu1/db9s/tree/main/crates/db9s-core/src/themes).
