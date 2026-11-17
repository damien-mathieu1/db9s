---
description: "Edit, insert and delete rows and documents in db9s, inline or in your $EDITOR."
---

# Editing

![editing](/gif/editing.gif)

## Field by field: `e`

`e` opens the selected row in the field editor: `j`/`k` pick a field, `enter`
edits it with a cursor, `enter` again applies, `ctrl+s` saves the row in one
update, `esc` discards. Changed fields are marked `●`.

Values keep their type: untouched text stays what it was, and typing `42`,
`true`, `null` or `{...}` gives a number, a boolean, null or an object.
Wrap in quotes (`"42"`) to force text.

![inline edit](/img/edit.png)

## In your editor: `E`

`E` writes the row as JSON to a temp file and opens your editor (`editor` in the
config, else `$EDITOR`, else `vi`). Save and quit: db9s shows the diff and asks
before writing.

## Insert: `a`

`a` opens the field editor on a blank row with the table's columns. Fields left
`null` are not sent, so the database fills them: serial ids, defaults, Mongo's
`_id`. `ctrl+s` inserts.

## Delete: `d`

`d` asks before deleting the selected row; the confirmation shows the exact
statement that will run, for example:

```sql
DELETE FROM "public"."users" WHERE "id" = 3
```

For SQL it is built by the same code as the executed statement, with the values
written in instead of bind parameters. The `E` diff confirm shows the `UPDATE`
the same way.

## How rows are found

| Backend | Edit / delete by |
|---|---|
| Postgres, MySQL, SQLite | the table's primary key (composite keys work); tables without one are read-only |
| MongoDB | `_id` |
| Redis | depends on the key type, see [Redis](../databases/redis.md) |
| RabbitMQ | messages are read-only; `d` purges the whole queue |
