---
description: "See a table's columns and types, or the fields of a Mongo collection."
---

# Schema

`i` on a table or collection shows its columns; `esc` closes.

![schema](/gif/schema.gif)

| Backend | Shows |
|---|---|
| Postgres, MySQL | `information_schema`: column, type, nullable, default, primary key |
| SQLite | `pragma_table_info`: same columns |
| MongoDB | the fields of the first 100 documents, the JSON types seen and how many documents have each (`in: 37/100`) |
