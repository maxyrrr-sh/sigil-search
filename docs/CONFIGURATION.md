# Sigil Search — configuration reference

Configuration is **declarative YAML** and is the source of truth. Validate it with:

```bash
sigil-search config validate ./configs/sigil-search.yaml
```

A complete example lives in [`configs/sigil-search.yaml`](../configs/sigil-search.yaml);
the Docker variant (binds `0.0.0.0`) is [`deploy/sigil-docker.yaml`](../deploy/sigil-docker.yaml).

## Top level

```yaml
version: 1            # config schema version (must be 1)
cluster: { … }
inputs: [ … ]
pipelines: [ … ]
index: { … }
api: { … }
query: { … }
plugins: [ … ]
```

## `cluster`

```yaml
cluster:
  targets: [all]            # any of: all | ingest | index | query | coordinator
  node_id: node-a           # optional; defaults to a process-derived id
  shards: 4                 # optional; index shard count (default 1)
  replicas: 2               # optional; replication factor (default 1)
  transport:
    kind: inproc            # inproc (default) | redpanda | kafka | nats
    brokers: ["redpanda:9092"]
  object_store: { kind: s3, bucket: sigil-cold, endpoint: http://minio:9000 }
```

`targets` selects which **roles** this node runs (see [OPERATIONS.md](OPERATIONS.md)).
`redpanda`/`kafka`/`nats` transports are not implemented yet — the node falls back
to the in-process bus with a warning.

## `inputs`

```yaml
inputs:
  - id: syslog_main
    type: syslog            # syslog | file | http_bulk | otlp
    listen: 0.0.0.0:5514    # network inputs
    codec: { type: syslog, rfc: 5424 }
  - id: applog
    type: file
    path: /var/log/app.ndjson
    codec: { type: json }
```

**Codecs:** `json`, `syslog`, `kv`, `cef` (no params); `csv` (`columns: [a,b]`);
`regex` / `grok` (`pattern: "…"`). See [PLUGINS.md](PLUGINS.md) for the codec set.

| Input `type` | Needs | Driven by | Runs the pipeline? |
|---|---|---|---|
| `syslog` | `listen` | runtime (UDP) | yes |
| `file` | `path` | runtime (one-shot) | yes |
| `http_bulk` | `listen` | serves the API router | no (direct index) |
| `otlp` | `listen` | serves the API router | no (direct index) |

## `pipelines`

A pipeline processes events from one or more inputs, then routes them.

```yaml
pipelines:
  - id: default
    from: [syslog_main, applog]   # input ids
    steps:
      - normalize: { schema: ecs }     # ECS is the only built-in schema
      - drain                          # Drain-style template mining
      - enrich: [geoip]                # geoip = IP scope classification
      - mask: { fields: [message], patterns: ["\\d{16}"] }  # PII redaction
      - set: { field: env, value: prod, when: "host.name contains prod" }
      - drop: { when: "log.level == debug" }
    route:
      - to: index                      # index | drop | deadletter
      - to: deadletter
        when: "log.level == error"
```

**Conditions** (`when`): `*` (always), `exists <field>`, `<field> == <value>`,
`<field> != <value>`, `<field> contains <value>`.

## `index`

```yaml
index:
  dir: ./data/index                       # hot Tantivy segments + catalog.json
  retention: { hot: 7d, warm: 30d, cold: 365d }
  rollover_secs: 3600                      # optional auto hot→cold rollover
```

**Durations:** `<n>{s|m|h|d|w}` — e.g. `30s`, `15m`, `24h`, `7d`, `2w`.
`retention.cold` drives cold-segment pruning. See tiering in [OPERATIONS.md](OPERATIONS.md).

## `api`

```yaml
api:
  listen: 0.0.0.0:9595      # the query/search API (default 127.0.0.1:9595)
```

## `query`

```yaml
query:
  languages: [sql, dsl, query_string]   # informational
```

## `plugins`

```yaml
plugins:
  - name: my_processor
    kind: wasm              # builtin | wasm | grpc
    path: ./plugins/my_processor.wasm
    capabilities: ["read:field:message", "emit:signal"]
```

Declared plugins are validated at startup (API version + capability grant) and
reported as *registered* / *pending* / *rejected*. `wasm`/`grpc` runtimes are
deferred (declared ones report *pending*). See [PLUGINS.md](PLUGINS.md).

## Validation rules

`config validate` checks: version is `1`; unique non-empty input ids; network
inputs have `listen`, file inputs have `path`; known input types and codecs;
pipelines reference declared inputs; cluster `targets` are valid roles.
