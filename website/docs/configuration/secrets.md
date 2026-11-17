---
description: "Keep database passwords out of the db9s config with env variables or any secret command."
---

# Secrets

A connection URL doesn't have to contain the password:

```toml
# environment variables, resolved when connecting
url = "postgres://app:${PG_PASSWORD}@db.internal:5432/app"

# or the output of any command: 1Password, pass, Vault, the macOS Keychain...
url = "cmd: op read op://infra/prod-db/url"
url = "cmd: security find-generic-password -s prod-db -w"
```

Only the template is saved and shown. A missing variable or a failing command
shows an error on that connection, and the others still work.

Commands run with no stdin: use tools whose prompt is graphical (1Password,
Touch ID, a graphical pinentry), not one that asks in the terminal.
