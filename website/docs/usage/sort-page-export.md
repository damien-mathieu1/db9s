---
description: "Sort rows server-side, page through big tables, and export to CSV or JSON."
---

# Sort, pages and export

![sort and export](/gif/sort-page-export.gif)

## Sort

`:sort <column>` orders the rows **on the server** (`ORDER BY` for SQL, a sort for
Mongo), so it's the real order of the whole table, not just the page on screen.
`:sort <column> desc` for descending, `:sort` alone back to the natural order. The
command bar completes column names.

Opening another table goes back to its natural order. Redis and RabbitMQ have no
column order.

## Pages

Rows come 200 at a time: `n` next page, `p` previous. When the backend can count
cheaply (SQL `COUNT(*)`, Mongo's collection metadata), the status shows the total
and the page:

```
200 rows (offset 400) of 1234, page 3/7, sorted by created_at ↓
```

## Export

`:export csv` or `:export json` writes the rows on screen to
`~/Downloads/<table>-<date>-<time>.csv|json` and shows the path. CSV follows RFC
4180 (quoted only when needed), objects and arrays are written as JSON.
