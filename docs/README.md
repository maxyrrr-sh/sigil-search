# Sigil Search — documentation

Start here. The top-level [README](../README.md) has the overview, roadmap, and
quickstart; these pages are the reference detail.

| Doc | What's in it |
|---|---|
| [API.md](API.md) | **HTTP API reference** — every endpoint, request/response, curl examples |
| [QUERY.md](QUERY.md) | The three query languages: query-string, SQL, pipe-DSL grammar |
| [CONFIGURATION.md](CONFIGURATION.md) | The declarative YAML config reference |
| [PLUGINS.md](PLUGINS.md) | Plugin API — extension traits, capabilities, lifecycle |
| [OPERATIONS.md](OPERATIONS.md) | CLI, roles, tiering, ports, Docker, seeding |
| [DESIGN.md](DESIGN.md) | Full architecture/design document (written in Ukrainian) |

## Architecture in one screen

```
            inputs                 pipeline                index            query
  syslog ─┐                ┌─ normalize (ECS)      ┌─ hot: Tantivy ─┐   /search (Lucene)
  file  ──┼─ codec ─ Record┼─ drain (templates)    │                ├─► /sql    (DataFusion)
  _bulk ──┤  (json/syslog/ ┼─ enrich (geoip)       └─ cold: Parquet ┘   /query  (pipe-DSL)
  otlp  ──┘   kv/cef/…)    ┼─ mask / set / drop          + catalog
                           └─ route ─► index / deadletter / drop
```

- **One binary, multiple roles** (`ingest`/`index`/`query`/`coordinator`) selected
  by config; monolith in-process, scale-out over a bus. ([OPERATIONS.md](OPERATIONS.md))
- **ECS-normalized `Event`** is the contract between every crate and plugin.
- **Own indexer:** Tantivy (full-text) + Arrow/DataFusion (analytics) + hot→cold
  Parquet tiering with a catalog.
- **Plugins** are how it grows into a SIEM ([PLUGINS.md](PLUGINS.md), [`siem/`](../siem/)).

## Crate map

See the table in [CLAUDE.md](../CLAUDE.md) / [README](../README.md). The web UI
lives in [`frontend/`](../frontend/), the SIEM example in [`siem/`](../siem/), and
deployment in [`deploy/`](../deploy/).

## Where things are

| Path | What |
|---|---|
| `crates/` | the Rust workspace (platform crates) |
| `frontend/` | the EUI web UI (Splunk-style search) |
| `siem/` | reference SIEM distribution (plugins on the core) |
| `configs/`, `deploy/` | example config + Docker compose |
| `samples/` | sample dataset + loader |
