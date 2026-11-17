---
description: "MongoDB in db9s: databases, collections, documents, filters and pipelines."
---

# MongoDB

```bash
db9s --mongo mongodb://localhost:27017
```

Databases are **databases**, containers are **collections**, rows are
**documents**. The rows pane shows the union of the fields of the page.

## Queries

`/` takes either:

- a find filter: `{"active": true, "age": {"$gt": 30}}` (500 documents at most);
- an aggregation pipeline, as a JSON array:
  `[{"$match": {"active": true}}, {"$group": {"_id": "$country", "n": {"$sum": 1}}}]`.

## Editing and types

Documents are found by `_id` (ObjectIds work). `ObjectId`, dates and `Decimal128`
show as text, binary as `<binary N bytes>`.

## Schema

`i` samples the first 100 documents: each field, the JSON types seen and how many
documents have it.

On a read-only connection, pipelines with `$out` or `$merge` are refused.
