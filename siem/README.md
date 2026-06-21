# sigil-siem — SIEM as plugins (Phase 7 proof)

This is a small **reference SIEM distribution** that runs on an **unmodified**
Sigil Search core. It is an independent Cargo workspace (not a member of the
sigil-search workspace) that depends on the platform crates **by path** and adds
all security behavior purely through the `sigil-core` extension traits.

> **The point:** prove the platform's plugin API (Phase 6) is strong enough to
> build a SIEM **without forking the core** — exactly the Phase 7 exit criterion,
> *"sigil-siem runs on an unmodified Sigil Search core."* In production this would
> be its own repository.

## What it adds (all as plugins)

| Crate | Extension point (`sigil-core` trait) | Role |
|---|---|---|
| `siem-ocsf` | `Schema` | Normalize to **OCSF** instead of ECS |
| `siem-sigma` | `Detector` | **Sigma** rule engine (YAML rules in `rules/`) |
| `siem-alert` | `Output` | Alerting sink (requests the `network` capability) |
| `siem-correlate` | `StorageBackend` + a `Correlator` | Brute-force correlation + provenance graph |
| `siem-cli` | — | The `sigil-siem` binary that assembles them on the core |

## Run

```bash
cargo run --manifest-path siem/Cargo.toml -p siem-cli
# or, from inside siem/:  cargo run -p siem-cli
```

It runs an end-to-end demo over a sample log burst:

```
raw syslog → platform `syslog` codec → OCSF Schema plugin → Sigma Detector plugin
          → alerting Output plugin + correlation + provenance graph
```

and prints the OCSF event, the detections, the escalated correlation, and the
graph — demonstrating, among other things, that the platform's **safe-default
capability grant denies** the alert plugin's `network` request until the SIEM
distribution explicitly grants it.

## What this is NOT

This reference pack proves *sufficiency of the plugin API*. The full enterprise
SIEM vision (semantic + causal correlation, embeddings/GNN via an ML sidecar,
broad ATT&CK mapping, OCSF breadth) is a much larger, separate effort — see the
standalone `sigil-siem` project. Nothing here modifies the `sigil-*` core.
