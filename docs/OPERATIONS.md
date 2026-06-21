# Sigil Search — operations

## CLI

```text
sigil-search <command> [args]

run [config]              Run the node (roles from config; default config: ./configs/sigil-search.yaml)
config validate [file]    Validate configuration
replay <file> [config]    Replay newline-delimited events through the pipeline (offline)
tier roll [config]        Roll the hot tier to a cold Parquet segment (offline)
tier prune [config]       Delete cold segments older than the cold retention (offline)
cluster info [config]     Show this node's roles, shard map, and membership
version                   Print version
```

> **Offline tools** (`replay`, `tier roll`/`prune`) open the index for writing, so
> they require the server to be **stopped** (one Tantivy writer per index dir).

## Roles (modular monolith → cluster)

`cluster.targets` selects which roles a node runs. In monolith mode (`[all]`)
they run in-process over the in-process bus; in scale-out, nodes run subsets.

| Role | Responsibility |
|---|---|
| `ingest` | run inputs + the processing pipeline; publish events to the bus |
| `index` | consume the bus, write/serve the index + tiers, run rollover |
| `query` | serve the search/SQL/DSL API (`api.listen`) |
| `coordinator` | membership, shard map, registries |

```bash
sigil-search cluster info ./configs/sigil-search.yaml      # roles + shard map (JSON)
# query-only node:  cluster: { targets: [query] }
```

Events flow `ingest → transport (bus) → index`. The bus is in-process today;
Kafka/Redpanda is the deferred scale-out backend.

## Tiering (hot → cold)

- **Hot:** recent events in Tantivy (full-text searchable).
- **Cold:** rolled-over Parquet segments under `<index.dir>/cold/`, tracked in
  `<index.dir>/catalog.json` (id, time range, count). Queryable via SQL/DSL.

```bash
sigil-search tier roll   ./cfg.yaml   # snapshot hot → a cold Parquet segment, clear hot
sigil-search tier prune  ./cfg.yaml   # delete cold segments older than retention.cold
```

Or set `index.rollover_secs` for automatic rollover + pruning while running.
**Note:** query-string (`/search`) is hot-only; SQL/DSL (`/sql`, `/query`) span
hot + cold.

## Ports

| Port | Service |
|---|---|
| `9595` | query/search API (`api.listen`) |
| `9200` | Elasticsearch-compatible `_bulk` / `_search` (http_bulk input) |
| `4317` | OTLP/HTTP logs (otlp input) |
| `5514/udp` | syslog input |

## Run with Docker

```bash
docker compose -f deploy/docker-compose.yml up --build
#  UI  -> http://localhost:8080
#  API -> http://localhost:9595   (_bulk :9200, OTLP :4317, syslog udp :5514)
```

The optional `full` profile adds Redpanda + MinIO (declared scale-out transport +
cold-tier object store, not yet consumed):

```bash
docker compose -f deploy/docker-compose.yml --profile full up --build
```

## Seeding data

```bash
# Elasticsearch _bulk
printf '%s\n' '{"index":{"_index":"web"}}' '{"message":"hello","host":{"name":"web1"}}' \
  | curl -s :9200/_bulk -H 'Content-Type: application/x-ndjson' --data-binary @-

# or replay a file (offline; server stopped)
sigil-search replay samples/events.ndjson ./configs/sigil-search.yaml
```

A ready-made dataset + loader lives in [`samples/`](../samples/).

## Observability

The node logs its topology, role activity, plugin lifecycle, rollovers, and
dead-letter/signal counts at startup and shutdown. `GET /healthz` is the liveness
probe (used by the Docker healthcheck).
