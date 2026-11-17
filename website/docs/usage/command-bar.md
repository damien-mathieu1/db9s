---
description: "The k9s-style : command bar: jump anywhere by name, sort, export, switch themes."
---

# Command bar

`:` opens a prompt to jump anywhere by name, with fish-style completion: the
rest of the best match shows greyed, `tab` or `→` takes it, `↑`/`↓` cycle
through the other matches.

![command bar](/gif/command-bar.gif)

| Command | Action |
|---|---|
| `:<connection>` | open that connection |
| `:<database>` | open that database / schema / vhost |
| `:<table>` | open that table / collection / key / queue, rows focused |
| `:sort <column> [desc]`, `:sort` | [sort](sort-page-export.md) server-side, or back to natural order |
| `:export csv`, `:export json` | [export](sort-page-export.md) the rows on screen |
| `:theme <name>` | switch [theme](../configuration/themes.md) (saved) |
| `:connections` (`:ctx`) | back to the connections |
| `:settings`, `:add` | settings, add a connection |
| `:q` | quit |

Names come from what's already listed: connections, plus the databases and
containers of the connection you're in.
