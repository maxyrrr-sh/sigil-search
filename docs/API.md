# Sigil Search — HTTP API reference

The HTTP API is served by `sigil-api` and exposed by the query role (`api.listen`,
default `127.0.0.1:9595`). The ecosystem inputs (`http_bulk`, `otlp`) serve the
**same router** on their own listen ports (e.g. `:9200`, `:4317`), so `_bulk`,
`_search`, `/v1/logs`, and the native query endpoints are reachable on any of
them.

- **Base URL:** `http://<api.listen>` (examples use `http://localhost:9595`).
- **Auth:** none yet (run behind a reverse proxy / network policy).
- **CORS:** not set by the server; the web UI uses a same-origin proxy.
- **Timestamps:** `ts` and `ingest_ts` are **epoch microseconds** (`ms = ts/1000`).
- **Errors:** failures return HTTP `500` with a JSON body `{"error": "<message>"}`.

| Method | Path | Purpose |
|---|---|---|
| `GET` | [`/healthz`](#get-healthz) | Liveness probe |
| `GET` | [`/search`](#get-search) | Full-text search (Lucene-style query string) |
| `GET` | [`/sql`](#get-sql) | SQL analytics (DataFusion) over the `events` table |
| `GET` | [`/query`](#get-query) | Pipe-DSL (SPL/KQL-style), lowered to SQL |
| `POST` | [`/_bulk`](#post-_bulk) | Elasticsearch-compatible bulk ingest |
| `GET`/`POST` | [`/_search`](#get-post-_search) | Minimal Elasticsearch-compatible search |
| `POST` | [`/v1/logs`](#post-v1logs) | OTLP/HTTP (JSON) log ingest |

---

## `GET /healthz`

Liveness check. Returns `200 OK` with the body `ok`.

```bash
curl http://localhost:9595/healthz        # -> ok
```

---

## `GET /search`

Full-text search via the Tantivy index. Supports Lucene-style query strings:
bare terms, `field:value`, boolean `AND`/`OR`/`NOT`, quoted phrases. Searches the
`message`, `host`, and an all-fields catch-all by default.

**Query parameters**

| Param | Type | Default | Notes |
|---|---|---|---|
| `q` | string | `""` | Query string. Empty → match-all (most recent). |
| `limit` | int | `20` | Max hits. |

**Response**

```jsonc
{
  "query": "Failed",
  "count": 2,
  "hits": [
    {
      "score": 1.64,
      "id": "1781897789895963-0",
      "ts": 1781897789895963,        // epoch microseconds
      "ingest_ts": 1781897789895963,
      "dataset": "web",
      "fields": [                     // array of [key, value] string pairs
        ["event.dataset", "web"],
        ["message", "Failed password for root"],
        ["host.name", "web1"],
        ["log.level", "error"]
      ]
    }
  ]
}
```

```bash
curl 'http://localhost:9595/search?q=Failed&limit=10'
curl 'http://localhost:9595/search?q=host:web1%20AND%20log.level:error'
```

---

## `GET /sql`

Analytical SQL over the `events` table, executed by DataFusion across the hot
(Tantivy) tier and any registered cold Parquet segments.

**The `events` table schema**

| Column | Type | Notes |
|---|---|---|
| `ts` | bigint | event time, epoch microseconds |
| `ingest_ts` | bigint | ingest time, epoch microseconds |
| `id` | text | document id |
| `dataset` | text | source feed / index name |
| `message` | text | ECS `message` |
| `host` | text | ECS `host.name` |
| `log_level` | text | ECS `log.level` |

**Query parameters:** `q` — the SQL statement.

**Response**

```jsonc
{ "count": 2, "rows": [ { "log_level": "error", "c": 2 }, { "log_level": "info", "c": 5 } ] }
```

```bash
curl -G 'http://localhost:9595/sql' \
  --data-urlencode 'q=SELECT log_level, count(*) AS c FROM events GROUP BY log_level ORDER BY c DESC'
```

> Time filtering is done in SQL (`WHERE ts BETWEEN <from_us> AND <to_us>`).

---

## `GET /query`

Pipe-DSL (SPL/KQL-style) lowered to SQL and run by the same engine. See
[QUERY.md](QUERY.md) for the full grammar.

**Query parameters:** `q` — the pipe-DSL expression.

**Response** — like `/sql`, plus the original `dsl` and the lowered `sql`:

```jsonc
{
  "dsl": "search login | stats count by host",
  "sql": "SELECT host, count(*) as count FROM events WHERE message LIKE '%login%' GROUP BY host",
  "count": 2,
  "rows": [ { "host": "web3", "count": 1 }, { "host": "app2", "count": 1 } ]
}
```

```bash
curl -G 'http://localhost:9595/query' --data-urlencode 'q=search error | stats count by host'
```

---

## `POST /_bulk`

Elasticsearch-compatible bulk ingest. Body is newline-delimited JSON (NDJSON):
an action line followed (for `index`/`create`/`update`) by a source line.
`Content-Type: application/x-ndjson`. The `_index` becomes the event `dataset`.
Documents are ECS-normalized and indexed. (Ecosystem ingest currently bypasses
the configurable processing pipeline.)

```bash
printf '%s\n' \
  '{"index":{"_index":"web"}}' \
  '{"message":"GET /login 200","host":{"name":"web1"},"source":{"ip":"10.0.0.5"}}' \
  '{"index":{"_index":"web"}}' \
  '{"message":"POST /login 401","host":{"name":"web2"}}' \
| curl -s -XPOST 'http://localhost:9200/_bulk' \
    -H 'Content-Type: application/x-ndjson' --data-binary @-
```

**Response**

```jsonc
{
  "took": 0,
  "errors": false,
  "items": [ { "index": { "_index": "web", "_id": null, "status": 201, "result": "created" } } ]
}
```

`delete` actions are accepted (status 200) but not applied (append-only store).

---

## `GET`/`POST` `/_search`

Minimal Elasticsearch-compatible search. Enough for simple shippers/tools; not a
full ES surface (no aggregations, `_field_caps`, version handshake, etc. — see
the README note on Kibana).

- **GET:** `?q=<query_string>&size=<n>`
- **POST:** JSON body with a supported subset of the ES query DSL:
  `match_all`, `query_string.query`, or `match.<field>`. Optional `size`.

```bash
curl -s -XPOST 'http://localhost:9200/_search' -H 'Content-Type: application/json' \
  -d '{"query":{"query_string":{"query":"login"}},"size":10}'
```

**Response** (ES-shaped)

```jsonc
{
  "took": 0,
  "timed_out": false,
  "hits": {
    "total": { "value": 1, "relation": "eq" },
    "max_score": 1.2,
    "hits": [ { "_index": "web", "_id": "…", "_score": 1.2, "_source": { "message": "POST /login 401", "host": { "name": "web2" } } } ]
  }
}
```

`_source` is reconstructed as a nested object from the event's dotted fields.

---

## `POST /v1/logs`

OTLP/HTTP logs receiver, **JSON** encoding (`application/json`). Accepts an OTLP
`ExportLogsServiceRequest`; each `logRecord` is mapped to an event
(`body.stringValue` → `message`, `severityText` → `log.level`, resource + record
attributes → fields), normalized to ECS under dataset `otlp`, and indexed. The
protobuf/gRPC encoding is not yet implemented.

```bash
curl -s -XPOST 'http://localhost:4317/v1/logs' -H 'Content-Type: application/json' -d '{
  "resourceLogs":[{"resource":{"attributes":[{"key":"service.name","value":{"stringValue":"checkout"}}]},
  "scopeLogs":[{"logRecords":[{"severityText":"ERROR","body":{"stringValue":"payment failed"},
  "attributes":[{"key":"order.id","value":{"intValue":"42"}}]}]}]}]}'
```

**Response**

```jsonc
{ "partialSuccess": { "rejectedLogRecords": 0, "acceptedLogRecords": 1 } }
```

---

## Notes & limitations

- **Event time = ingest time.** The default schema sets `ts` to ingest time;
  timestamp parsing from the log is a planned enhancement, so histograms cluster
  near "now".
- **No aggregations in `/_search`.** Use `/sql` or `/query` for analytics.
- **Pipeline bypass.** `_bulk` and `/v1/logs` normalize to ECS and index directly;
  the configurable codec/enrich/route pipeline applies to the `syslog`/`file`
  inputs driven by the runtime.
- **Single-writer.** Offline tools (`replay`, `tier roll`) need the server stopped
  (one Tantivy writer per index directory).
