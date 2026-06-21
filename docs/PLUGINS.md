# Sigil Search — plugin API

Plugins are the extension mechanism. The platform stays general; domain behavior
(e.g. a SIEM) is added purely as plugins — see [`siem/`](../siem/) for a worked
example that builds OCSF + Sigma + alerting + correlation on the **unmodified**
core.

## Extension points (`sigil-core` traits)

Every plugin implements `Plugin` (returns a `PluginManifest`) plus one capability
trait:

| Trait | Method | Role |
|---|---|---|
| `Input` | `poll()` | source of raw bytes |
| `Codec` | `decode(&[u8]) -> Vec<Record>` | bytes → records |
| `Schema` | `normalize(Record) -> Event` | records → normalized events (ECS/OCSF) |
| `Processor` | `process(Event) -> Vec<Event>` | map / filter / enrich |
| `Detector` | `eval(&Event) -> Option<Signal>` | detection (the SIEM hook) |
| `Output` | `emit(&[u8])` | send to an external sink |
| `StorageBackend` | `flush()` | pluggable storage (e.g. a graph store) |
| `QueryFn` | `name()` | user-defined query function |

The built-in codecs (`json`, `syslog`, `kv`, `cef`, `csv`, `regex`, `grok`), the
ECS `Schema`, the pipeline `Processor`s (drain/geoip/mask/lookup), and an example
`Detector` are all first-party plugins registered through the same host.

## The host (`sigil-plugin`)

`PluginHost` is a typed registry that enforces policy on registration:

- **Capabilities** — a plugin's manifest *requests* capabilities; the host
  *grants* a set. Registration fails if a request is not granted.
  Tokens: `read:field:<name>` / `read:field:*`, `write:field:<name>`,
  `emit:signal`, `network`. The safe default grants field read/write + signals
  but **denies `network`** (an alerting plugin must be granted it explicitly).
- **Versioning** — `ApiVersion` (`major.minor`). A plugin is compatible when the
  major matches and the host minor ≥ the plugin's.
- **Manifest** — validated; a content `digest` is available to pin against
  (cryptographic signing is deferred).
- **Contracts** — `contracts::codec_contract` / `detector_contract` are the
  conformance checks the ecosystem runs against a plugin.

## Lifecycle (declared plugins)

Plugins declared in config (`plugins:`) are evaluated at startup and reported:

| State | Meaning |
|---|---|
| `registered` | builtin found + capabilities granted + version compatible |
| `pending` | valid, but its runtime is not implemented yet (`wasm` / `grpc`) |
| `rejected` | bad version, ungranted capability, or unknown kind/name |

```
[sigil-search] plugin: error-level (builtin) registered
[sigil-search] plugin: my_geo (wasm) pending: wasm component host not yet available
[sigil-search] plugin: exfil (grpc) rejected: ungranted capability 'network'
```

## Plugin kinds

| Kind | Status |
|---|---|
| `builtin` (compile-time trait object) | **working** — resolved from the registry |
| `wasm` (wasmtime Component Model) | interface + lifecycle done; runtime deferred |
| `grpc` (sidecar + Arrow Flight) | interface + lifecycle done; runtime deferred |

## Building a distribution (the SIEM example)

`siem/` is an independent workspace that depends on the platform crates by path
and adds, as plugins only:

| Crate | Trait | Adds |
|---|---|---|
| `siem-ocsf` | `Schema` | OCSF normalization |
| `siem-sigma` | `Detector` | Sigma rule engine (YAML rules) |
| `siem-alert` | `Output` | alerting (requests `network`) |
| `siem-correlate` | `StorageBackend` + a `Correlator` | correlation + provenance graph |

Run it: `cargo run --manifest-path siem/Cargo.toml -p siem-cli`.
