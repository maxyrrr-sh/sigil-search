//! `sigil-config` — declarative configuration: load, validate (DESIGN §12).
//!
//! Phase 0 scope: parse the YAML node config into a typed [`Config`] and run
//! structural + referential validation. `plan`/`apply`/drift land in later
//! phases; the types here are the source of truth they will diff against.
#![allow(dead_code)]

use std::path::Path;

use serde::Deserialize;

/// Root of a Sigil Search node configuration (`configs/sigil-search.yaml`).
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Config schema version (currently `1`).
    pub version: u32,
    #[serde(default)]
    pub cluster: Cluster,
    #[serde(default)]
    pub inputs: Vec<Input>,
    #[serde(default)]
    pub pipelines: Vec<Pipeline>,
    #[serde(default)]
    pub index: Index,
    #[serde(default)]
    pub api: Api,
    #[serde(default)]
    pub query: Query,
    #[serde(default)]
    pub plugins: Vec<Plugin>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Cluster {
    #[serde(default = "default_targets")]
    pub targets: Vec<String>,
    /// Stable node identifier (defaults to a process-derived id at runtime).
    #[serde(default)]
    pub node_id: Option<String>,
    /// Number of index shards (default 1).
    #[serde(default)]
    pub shards: Option<u32>,
    /// Replication factor (default 1).
    #[serde(default)]
    pub replicas: Option<u32>,
    #[serde(default)]
    pub object_store: Option<serde_yaml::Value>,
    #[serde(default)]
    pub transport: Option<serde_yaml::Value>,
}

impl Cluster {
    /// Transport kind from `cluster.transport.kind` (default `inproc`).
    pub fn transport_kind(&self) -> String {
        self.transport
            .as_ref()
            .and_then(|v| v.get("kind"))
            .and_then(|v| v.as_str())
            .unwrap_or("inproc")
            .to_string()
    }
}

fn default_targets() -> Vec<String> {
    vec!["all".to_string()]
}

const VALID_ROLES: [&str; 5] = ["all", "ingest", "index", "query", "coordinator"];

/// An ingest source (DESIGN §6). `kind` mirrors the YAML `type:` key.
#[derive(Debug, Clone, Deserialize)]
pub struct Input {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// `host:port` for network inputs (syslog/http_bulk/otlp).
    #[serde(default)]
    pub listen: Option<String>,
    /// Filesystem path for `file` inputs.
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub codec: Option<Codec>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Codec {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub rfc: Option<u32>,
    /// Pattern for the `regex` / `grok` codecs.
    #[serde(default)]
    pub pattern: Option<String>,
    /// Column names for the `csv` codec.
    #[serde(default)]
    pub columns: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Pipeline {
    pub id: String,
    #[serde(default)]
    pub from: Vec<String>,
    #[serde(default)]
    pub steps: Vec<serde_yaml::Value>,
    #[serde(default)]
    pub route: Vec<serde_yaml::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Index {
    #[serde(default)]
    pub retention: Option<serde_yaml::Value>,
    /// Local directory for hot Tantivy segments (Phase 0). Defaults to `./data/index`.
    #[serde(default = "default_index_dir")]
    pub dir: String,
    /// If set, automatically roll the hot tier to cold every N seconds (Phase 2).
    #[serde(default)]
    pub rollover_secs: Option<u64>,
}

impl Default for Index {
    fn default() -> Self {
        Index {
            retention: None,
            dir: default_index_dir(),
            rollover_secs: None,
        }
    }
}

/// Parse a duration like `30s`, `15m`, `24h`, `7d`, `2w` into seconds.
pub fn parse_duration(s: &str) -> Option<u64> {
    let s = s.trim();
    let split = s.find(|c: char| !c.is_ascii_digit())?;
    let (num, unit) = s.split_at(split);
    let n: u64 = num.parse().ok()?;
    let mult = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        "w" => 604_800,
        _ => return None,
    };
    Some(n * mult)
}

fn default_index_dir() -> String {
    "./data/index".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct Api {
    #[serde(default = "default_api_listen")]
    pub listen: String,
}

impl Default for Api {
    fn default() -> Self {
        Api {
            listen: default_api_listen(),
        }
    }
}

fn default_api_listen() -> String {
    "127.0.0.1:9595".to_string()
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Query {
    #[serde(default)]
    pub languages: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Plugin {
    pub name: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl Config {
    /// Parse a config from a YAML string.
    pub fn from_yaml(yaml: &str) -> anyhow::Result<Self> {
        let cfg: Config = serde_yaml::from_str(yaml)?;
        Ok(cfg)
    }

    /// Load and parse a config file from disk.
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
        Self::from_yaml(&text).map_err(|e| anyhow::anyhow!("parsing {}: {e}", path.display()))
    }

    /// Structural + referential validation. Returns a list of human-readable
    /// problems; an empty list means the config is valid.
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        if self.version != 1 {
            errors.push(format!(
                "unsupported config version {} (expected 1)",
                self.version
            ));
        }

        // Cluster role targets must be recognized.
        for target in &self.cluster.targets {
            if !VALID_ROLES.contains(&target.as_str()) {
                errors.push(format!(
                    "unknown cluster target '{target}' (valid: {})",
                    VALID_ROLES.join(", ")
                ));
            }
        }

        // Input ids must be unique and non-empty.
        let mut seen = std::collections::HashSet::new();
        for input in &self.inputs {
            if input.id.is_empty() {
                errors.push("input with empty id".to_string());
            } else if !seen.insert(input.id.as_str()) {
                errors.push(format!("duplicate input id: {}", input.id));
            }
            // Network inputs need a listen address; file inputs need a path.
            match input.kind.as_str() {
                "syslog" | "http_bulk" | "otlp" => {
                    if input.listen.is_none() {
                        errors.push(format!("input '{}' ({}) needs a listen address", input.id, input.kind));
                    }
                }
                "file" => {
                    if input.path.is_none() {
                        errors.push(format!("input '{}' (file) needs a path", input.id));
                    }
                }
                other => {
                    errors.push(format!("input '{}' has unknown type '{}'", input.id, other));
                }
            }
        }

        // Pipelines must reference declared inputs.
        for p in &self.pipelines {
            for src in &p.from {
                if !self.inputs.iter().any(|i| &i.id == src) {
                    errors.push(format!(
                        "pipeline '{}' references unknown input '{}'",
                        p.id, src
                    ));
                }
            }
        }

        errors
    }

    /// Inputs that Phase 0 can actually run (`syslog`, `file`); others are
    /// recognized but not yet wired (Phase 4 ecosystem inputs).
    pub fn runnable_inputs(&self) -> impl Iterator<Item = &Input> {
        self.inputs
            .iter()
            .filter(|i| matches!(i.kind.as_str(), "syslog" | "file"))
    }

    /// Cold-tier retention in seconds, from `index.retention.cold` if present.
    pub fn cold_retention_secs(&self) -> Option<u64> {
        self.index
            .retention
            .as_ref()
            .and_then(|v| v.get("cold"))
            .and_then(|v| v.as_str())
            .and_then(parse_duration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../../../configs/sigil-search.yaml");

    #[test]
    fn parses_and_validates_sample_config() {
        let cfg = Config::from_yaml(SAMPLE).expect("sample config should parse");
        assert_eq!(cfg.version, 1);
        assert!(cfg.inputs.iter().any(|i| i.id == "syslog_main"));
        let errs = cfg.validate();
        assert!(errs.is_empty(), "unexpected validation errors: {errs:?}");
    }

    #[test]
    fn flags_unknown_input_reference() {
        let yaml = r#"
version: 1
inputs:
  - id: a
    type: syslog
    listen: 0.0.0.0:5514
pipelines:
  - id: p
    from: [does_not_exist]
"#;
        let cfg = Config::from_yaml(yaml).unwrap();
        let errs = cfg.validate();
        assert!(errs.iter().any(|e| e.contains("unknown input")), "{errs:?}");
    }
}
