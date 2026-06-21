# Sigil Search — query languages

The same data is queryable three ways over one engine. Pick the one that fits;
the web UI lets you toggle between them.

| Language | Endpoint | Best for |
|---|---|---|
| Query-string (Lucene-style) | `GET /search` | fast full-text lookups |
| SQL (DataFusion) | `GET /sql` | analytics, aggregations, joins-to-cold |
| Pipe-DSL (SPL/KQL-style) | `GET /query` | Splunk-style "search then transform" |

---

## Query-string (full-text)

Lucene-style strings evaluated by Tantivy. Default fields: `message`, `host`, and
an all-fields catch-all.

```
Failed password                  # all terms
host:web1                        # field match
log.level:error AND message:db   # boolean
"connection refused"             # phrase
status:(500 OR 503)              # grouping
```

Empty query → match-all (most recent first).

---

## SQL (DataFusion)

Standard SQL over the `events` table (hot + cold tiers unioned):

| Column | Type |
|---|---|
| `ts`, `ingest_ts` | bigint (epoch microseconds) |
| `id`, `dataset`, `message`, `host`, `log_level` | text |

```sql
SELECT log_level, count(*) AS c FROM events GROUP BY log_level ORDER BY c DESC;
SELECT host, count(*) AS hits FROM events WHERE message LIKE '%Failed%' GROUP BY host;
SELECT * FROM events WHERE ts BETWEEN 1781890000000000 AND 1781899999999999 LIMIT 100;
SELECT (ts/3600000000) AS hour_bucket, count(*) FROM events GROUP BY hour_bucket ORDER BY hour_bucket;
```

The full DataFusion SQL dialect is available (aggregations, `CASE`, window
functions, etc.). Time filtering is expressed directly with `WHERE ts …`.

---

## Pipe-DSL (Splunk/KQL-style)

A small `a | b | c` pipeline that **lowers to SQL** (the lowered SQL is returned
in the `/query` response, so you can learn the mapping). Stages run left to right.

### Commands

| Stage | Example | Lowers to |
|---|---|---|
| `search <text>` | `search Failed password` | `WHERE message LIKE '%Failed password%'` |
| `where <expr>` | `where log.level == error` | `WHERE log_level = 'error'` |
| `stats count [by <field>]` | `stats count by host` | `SELECT host, count(*) AS count … GROUP BY host` |
| `fields <a,b>` | `fields host, message` | `SELECT host, message` |
| `sort <field> [desc\|asc]` | `sort count desc` | `ORDER BY count DESC` |
| `head <n>` / `limit <n>` | `head 20` | `LIMIT 20` |

### `where` operators

`==`, `!=`, `=` (equality), and `contains` (→ `LIKE '%…%'`). Values are quoted
automatically unless numeric.

### Field aliases

`log.level` → `log_level`; `host.name` / `host` → `host`. Other names pass through
to the column of the same name.

### Examples

```
search login | stats count by host
search error | where host.name == web1 | head 50
stats count by log_level | sort count desc
search Failed | fields host, message | head 100
```

### Limits

- `stats` currently supports **`count`** only (`count by <field>` or bare `count`).
- One `selection`/predicate per `where`; no range operators (`>`, `<`) yet — use
  SQL mode for ranges and richer aggregations.
- A query with no `stats` returns event rows (Discover view); a query with `stats`
  returns an aggregation table (Statistics view).
