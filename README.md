# Sigil Search

> **Status: Phases 0–4 complete; Phases 5–6 (scale-out, plugin API) partial.** A
> single binary ingests (syslog / file / ES `_bulk` / OTLP) through a configurable
> codec + ECS + enrich/mask/route pipeline, indexes with Tantivy, rolls hot→cold
> Parquet segments with a catalog, and answers full-text, SQL (DataFusion), and
> pipe-DSL queries over one engine. It runs **role-selected** (`ingest`/`index`/
> `query`) over a pluggable transport bus, and extends through a **plugin host**
> (typed registry + capability/version enforcement; an example detector emits
> signals). Remaining for true multi-node + third-party plugins: a Kafka/Redpanda
> transport, a Raft-replicated catalog, and the wasm/gRPC plugin runtimes.
> **Phase 7 is proven**: a reference SIEM ([`siem/`](siem/) — OCSF schema, Sigma
> detector, alerting, correlation + provenance graph) builds purely as plugins on
> the **unmodified** core.

An open-source **search & ingest platform** written in Rust — an **ELK analog**
(Elasticsearch + Logstash + Kibana) delivered as a single binary: its own
indexer, normalization, vertical + horizontal scaling, declarative-first
configuration, and a tiered plugin system.

**End goal:** a plugin system powerful enough to turn the platform into an
**enterprise SIEM**. That SIEM distribution is **`sigil-siem`** — a separate repo
that depends on these crates and adds security plugins (Sigma detection,
semantic + causal correlation, ATT&CK mapping).

## How it maps to ELK

| ELK | Sigil Search |
|---|---|
| Elasticsearch | `sigil-index` (Tantivy + Arrow/DataFusion) + `sigil-cluster` |
| Logstash | `sigil-ingest` + pipeline DAG + `sigil-schema` |
| Beats | input plugins; Elasticsearch-compatible `_bulk` |
| Kibana | `sigil-query` + `sigil-api` + **[`frontend/`](frontend/) — a Splunk-style EUI web UI** |

Closest prior art in Rust is **Quickwit** and **OpenObserve** — we follow the
same proven formula (Tantivy + object storage + ES-compat + role-based monolith)
and differentiate on declarative-first config and a SIEM-grade plugin API.

## Documentation

- **[docs/DESIGN.md](docs/DESIGN.md)** — full design: pipeline, schema, indexer,
  query, scaling, plugin system, and how a SIEM layers on top.
- **[CLAUDE.md](CLAUDE.md)** — contributor / agent guide and crate map.

## Layout

```
crates/    Rust workspace (crate map in CLAUDE.md)
configs/   Example declarative configuration
deploy/    docker-compose / Helm
plugins/   Example plugins
docs/      Design documentation
```

## Quickstart

```bash
cargo build
cargo run -p sigil-cli -- config validate ./configs/sigil-search.yaml
cargo run -p sigil-cli -- run            # inputs + pipeline + index + query API

# in another shell — multiple ways to get data in and query it back:

# 1. syslog over UDP, then full-text search
printf '<34>Oct 11 22:14:15 host sshd[1234]: Failed password for root' \
  | nc -u -w1 127.0.0.1 5514
curl 'http://127.0.0.1:9595/search?q=Failed'

# 2. Elasticsearch-compatible bulk ingest (port 9200)
printf '%s\n' '{"index":{"_index":"logs"}}' '{"message":"hello","host":{"name":"web1"}}' \
  | curl -s :9200/_bulk -H 'Content-Type: application/x-ndjson' --data-binary @-

# 3. SQL (DataFusion) and pipe-DSL over the same data
curl 'http://127.0.0.1:9595/sql?q=SELECT%20log_level,count(*)%20FROM%20events%20GROUP%20BY%20log_level'
curl 'http://127.0.0.1:9595/query?q=search%20Failed%20%7C%20stats%20count%20by%20host'
```

Other commands: `replay <file>` (feed a file through the pipeline),
`tier roll` / `tier prune` (hot→cold rollover + cold retention),
`cluster info` (roles, shard map, membership). Set `cluster.targets` in config to
run a node as a subset of roles, e.g. `targets: [query]` for a query-only node.

## Web UI

[`frontend/`](frontend/) is a **Splunk-style search UI in an Elastic/Kibana look**
(built with Elastic's EUI): one search bar with **Pipe-DSL / Lucene / SQL** modes
and a time picker, a Discover-style events view (histogram + expandable events +
field sidebar), a Statistics view (table + chart), and light/dark themes.

```bash
cd frontend && npm install && npm run dev   # http://localhost:5173 (Vite proxies /api -> the backend)
```

## Run the full stack (Docker)

```bash
docker compose -f deploy/docker-compose.yml up --build
#  UI  -> http://localhost:8080      (nginx serves the SPA + proxies /api -> backend)
#  API -> http://localhost:9595      (ES _bulk :9200, OTLP :4317, syslog udp :5514)
```

The optional `full` profile adds Redpanda + MinIO (the *declared* scale-out
transport and cold-tier object store — not yet consumed by the app):
`docker compose -f deploy/docker-compose.yml --profile full up --build`.

## Roadmap

Organized as **vertical slices**: each phase ships something runnable. The
platform is useful on its own by Phase 3; the plugin API (Phase 6) is what
unlocks the SIEM distribution (Phase 7).

Legend: ☐ planned · ◐ in progress · ☑ done. **Phases 0–4 are done**; **Phases 5–6
are partial** (role selection + in-proc bus; plugin host with compile-time
extension — multi-node and wasm/gRPC runtimes deferred). A few sub-items are
partial, noted per phase. **Phase 7 is proven** by an on-the-core reference plugin pack.

### Overview

| Phase | Theme | Exit milestone |
|------:|-------|----------------|
| ☑ 0 | Foundations | Events ingested, indexed, searchable; declarative config applies |
| ☑ 1 | Pipeline (Logstash analog) | Full ingest → parse → normalize → enrich → route |
| ☑ 2 | Indexer maturity | Long retention + analytics across hot/warm/cold tiers |
| ☑ 3 | Query | SQL + pipe-DSL + query-string over one engine |
| ☑ 4 | Ecosystem compatibility | Adopt without rewriting pipelines (ES `_bulk`, Beats, OTLP) |
| ◐ 5 | Scale-out | One binary: monolith → cluster |
| ◐ 6 | Plugin API | Stable, versioned, sandboxed plugin system |
| ☑ 7 | SIEM enablement | `sigil-siem` builds on top via plugins |

### Phase 0 — Foundations

**Goal:** a single binary that ingests, normalizes, indexes, and searches events, fully driven by declarative config.

- [x] `sigil-core`: `Event`/ECS model (field constants + accessors), plugin traits, error type
- [x] `sigil-config`: YAML load + structural/referential `validate` _(JSON Schema, `plan`/`apply` still ☐)_
- [x] `sigil-ingest`: `syslog` and `file` inputs; `json` + `syslog` codecs
- [x] `sigil-schema`: ECS mapping (`EcsSchema`) for syslog + json sources
- [x] `sigil-index`: Tantivy hot segments; write + full-text / field / match-all search
- [x] `sigil-api`: read-only `/search` endpoint; `sigil-cli`: `run` + `config validate`

**Exit:** `sigil-search run` ingests syslog, results are searchable, the node is defined in `configs/sigil-search.yaml`. ✅ **Achieved.**

### Phase 1 — Pipeline (Logstash analog)

**Goal:** a complete, configurable processing pipeline.

- [x] Codecs: `cef`, `kv`, `csv`, `regex`, `grok` (builtin pattern set)
- [x] Online template mining (Drain-style masking) → `template_id` + variables
- [x] ECS normalization breadth; schema-on-read fallback (unknown fields pass through)
- [~] Enrichment processors: GeoIP (DB-free scope classification) + asset/identity `lookup`; **reverse DNS still ☐**
- [x] Pipeline DAG + conditional routing; dead-letter handling; PII masking
- [ ] Hot-reload of safe config changes _(deferred)_

**Exit:** heterogeneous sources land normalized to ECS through a declarative DAG. ✅ **Achieved.**

### Phase 2 — Indexer maturity

**Goal:** real retention and analytics.

- [~] Tiered storage lifecycle: hot (Tantivy) → cold (Parquet) rollover _(warm tier not yet distinct)_
- [~] Cold tier in Parquet on local FS _(S3/MinIO via `object_store` still ☐)_
- [x] DataFusion analytical queries over Arrow/Parquet (cold unioned with hot)
- [x] Catalog (`catalog.json`) with segment metadata + cold pruning by age
- [x] Declarative retention / rollover policies (`index.rollover_secs`, `retention.cold`)

**Exit:** retention on cheap cold storage + analytics over history. ✅ **Achieved** (local-FS Parquet; S3 pending).

### Phase 3 — Query

**Goal:** ergonomic search and analytics.

- [x] SQL frontend (DataFusion) over the `events` table (hot + cold)
- [x] pipe-DSL (SPL/KQL-style) lowering to SQL — `search | where | stats | fields | sort | head`
- [x] Lucene-style query-string for full-text (Tantivy)
- [x] Unified `QueryEngine` routing by language; JSON API (`/sql`, `/query`, `/search`)

**Exit:** the same data is queryable via SQL, DSL, and query-string. ✅ **Achieved.**

### Phase 4 — Ecosystem compatibility

**Goal:** drop-in adoption.

- [x] Elasticsearch-compatible `_bulk` ingest endpoint
- [ ] Lumberjack (Beats) input _(deferred — binary framed protocol)_
- [x] OTLP logs input (OTLP/HTTP **JSON** at `/v1/logs`; protobuf/gRPC still ☐)
- [x] (Stretch) minimal ES `_search`-compatible endpoint (`match_all` / `query_string` / `match`)

**Exit:** existing Logstash/Fluent/OTel(HTTP) shippers send to Sigil Search unchanged. ✅ **Achieved** (Beats/Lumberjack pending).

### Phase 5 — Scale-out

**Goal:** deliver the "monolith that scales".

- [x] Role targets (`ingest`/`index`/`query`/`coordinator`) via config — gated at runtime
- [~] Transport abstraction (`Transport` trait + in-proc impl) wired into the ingest→index path; **Kafka/Redpanda impl deferred** (falls back to in-proc)
- [~] Membership + shard map + index registry data model (`ClusterCatalog`, `cluster info`); **Raft (`openraft`) replication deferred**
- [x] Index sharding (time + hash) + replica placement (`ShardMap`)

**Exit:** the same binary runs as a 3+ node cluster sharing object storage. ◐ **Partial** — one binary runs role subsets over an in-proc bus; true multi-node (Kafka transport + Raft catalog + shared object store + cross-node query) is the remaining work.

### Phase 6 — Plugin API

**Goal:** the stable extension surface — the foundation for SIEM and beyond.

- [x] Trait registry for compile-time (first-party) plugins (`PluginHost`); codecs resolve from it, an example `Detector` emits `Signal`s
- [~] `sigil-plugin` host **interface + lifecycle** (discover → validate → grant → register); **wasmtime Component Model runtime deferred** (declared `wasm` plugins validate → *pending*)
- [~] Capability-based permissions + manifest validation/digest **done**; **cryptographic signing deferred** (non-crypto digest for now)
- [~] gRPC + Arrow Flight sidecar **lifecycle** done; **runtime deferred** (declared `grpc` plugins → *pending*)
- [x] Versioned (`ApiVersion`) plugin contracts + contract-test helpers (`contracts::*`)

**Exit:** third parties can extend ingest/schema/detect/output without forking the core. ◐ **Partial** — compile-time extension works end-to-end (registry resolution + a detector emitting signals, with capability/version enforcement); the wasm + gRPC runtimes are the remaining work.

### Phase 7 — SIEM enablement

**Goal:** prove the end goal — a SIEM built purely as plugins.

- [x] Reference security plugin pack ([`siem/`](siem/) — an independent workspace on the unmodified core; a real distribution would be its own repo)
- [x] OCSF `Schema` plugin (`siem-ocsf`); Sigma `Detector` plugin (`siem-sigma`); alerting `Output` (`siem-alert`)
- [x] Plugin API validated for correlation + graph backend (`siem-correlate`: `Correlator` + a `GraphBackend` implementing `StorageBackend`)

**Exit:** `sigil-siem` runs on an unmodified Sigil Search core. ✅ **Achieved** — the `sigil-siem` binary builds and runs entirely on the unmodified `sigil-core`/`sigil-ingest`/`sigil-plugin`, with capability enforcement gating the alert plugin's `network` request. _(This is a reference pack proving plugin-API sufficiency; the full enterprise SIEM — semantic/causal correlation, ML sidecar, ATT&CK breadth — is a larger separate effort.)_

### Cross-cutting (every phase)

- [ ] **Security**: mTLS, RBAC, multitenancy, secrets, audit log, plugin sandbox
- [ ] **Observability**: Prometheus self-metrics, OTLP tracing, health endpoints
- [ ] **Quality**: CI (fmt, clippy, tests), `cargo deny`/`audit`, golden tests
- [ ] **Docs**: keep `docs/DESIGN.md` and ADRs in sync with the code

## License

Apache-2.0 (intended).
