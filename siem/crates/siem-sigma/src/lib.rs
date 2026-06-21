//! `siem-sigma` — a Sigma detection-rule [`Detector`] plugin.
//!
//! Sigma rules are loaded from YAML and evaluated against normalized
//! [`Event`]s. This is the SIEM's detection engine expressed entirely through
//! the platform's `sigil_core::Detector` extension point — the hook the core
//! advertises for exactly this purpose.
//!
//! Supported subset: a single `selection` map (keys ANDed) with value modifiers
//! `contains` / `startswith` / `endswith`, list values (ORed), and
//! `condition: selection`.

use serde::Deserialize;
use sigil_core::{Detector, Event, Plugin, PluginManifest, Signal};

/// A compiled Sigma rule.
#[derive(Debug, Clone)]
pub struct SigmaRule {
    pub title: String,
    pub level: String,
    pub technique: Option<String>,
    selectors: Vec<Selector>,
}

#[derive(Debug, Clone)]
struct Selector {
    field: String,
    op: MatchOp,
    values: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
enum MatchOp {
    Eq,
    Contains,
    StartsWith,
    EndsWith,
}

#[derive(Debug, Deserialize)]
struct RawRule {
    title: String,
    #[serde(default)]
    level: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    detection: serde_yaml::Value,
}

impl SigmaRule {
    /// Parse a single Sigma rule from a YAML document.
    pub fn from_yaml(yaml: &str) -> anyhow::Result<SigmaRule> {
        let raw: RawRule = serde_yaml::from_str(yaml)?;
        let selection = raw
            .detection
            .get("selection")
            .and_then(|v| v.as_mapping())
            .ok_or_else(|| anyhow::anyhow!("rule '{}' has no `detection.selection`", raw.title))?;

        let mut selectors = Vec::new();
        for (key, value) in selection {
            let key = key.as_str().unwrap_or_default();
            let (field, op) = parse_field(key);
            selectors.push(Selector {
                field,
                op,
                values: yaml_to_strings(value),
            });
        }

        let technique = raw.tags.iter().find_map(|t| {
            t.strip_prefix("attack.t")
                .map(|num| format!("T{}", num.to_ascii_uppercase()))
        });

        Ok(SigmaRule {
            title: raw.title,
            level: raw.level.unwrap_or_else(|| "medium".to_string()),
            technique,
            selectors,
        })
    }

    /// Does every selector match the event?
    pub fn matches(&self, event: &Event) -> bool {
        self.selectors.iter().all(|s| s.matches(event))
    }

    fn severity(&self) -> u8 {
        match self.level.as_str() {
            "critical" => 90,
            "high" => 70,
            "medium" => 50,
            "low" => 30,
            _ => 10,
        }
    }
}

impl Selector {
    fn matches(&self, event: &Event) -> bool {
        let Some(actual) = event.get(&self.field) else {
            return false;
        };
        let actual = actual.to_ascii_lowercase();
        self.values.iter().any(|v| {
            let v = v.to_ascii_lowercase();
            match self.op {
                MatchOp::Eq => actual == v,
                MatchOp::Contains => actual.contains(&v),
                MatchOp::StartsWith => actual.starts_with(&v),
                MatchOp::EndsWith => actual.ends_with(&v),
            }
        })
    }
}

fn parse_field(key: &str) -> (String, MatchOp) {
    match key.split_once('|') {
        Some((field, "contains")) => (field.to_string(), MatchOp::Contains),
        Some((field, "startswith")) => (field.to_string(), MatchOp::StartsWith),
        Some((field, "endswith")) => (field.to_string(), MatchOp::EndsWith),
        Some((field, _)) => (field.to_string(), MatchOp::Eq),
        None => (key.to_string(), MatchOp::Eq),
    }
}

fn yaml_to_strings(value: &serde_yaml::Value) -> Vec<String> {
    match value {
        serde_yaml::Value::Sequence(seq) => seq.iter().filter_map(scalar_string).collect(),
        other => scalar_string(other).into_iter().collect(),
    }
}

fn scalar_string(value: &serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// A [`Detector`] backed by a set of Sigma rules.
pub struct SigmaDetector {
    rules: Vec<SigmaRule>,
    manifest: PluginManifest,
}

impl SigmaDetector {
    pub fn new(rules: Vec<SigmaRule>) -> Self {
        SigmaDetector {
            rules,
            manifest: PluginManifest {
                name: "sigma".to_string(),
                version: "0.1.0".to_string(),
                capabilities: vec!["read:field:*".to_string(), "emit:signal".to_string()],
            },
        }
    }

    /// Load every `*.yml` / `*.yaml` rule in a directory.
    pub fn load_dir(dir: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        let mut rules = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            let is_yaml = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e == "yml" || e == "yaml")
                .unwrap_or(false);
            if is_yaml {
                let text = std::fs::read_to_string(&path)?;
                rules.push(SigmaRule::from_yaml(&text)?);
            }
        }
        rules.sort_by_key(|r| std::cmp::Reverse(r.severity()));
        Ok(SigmaDetector::new(rules))
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

impl Plugin for SigmaDetector {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
}

impl Detector for SigmaDetector {
    fn eval(&self, event: &Event) -> Option<Signal> {
        let rule = self.rules.iter().find(|r| r.matches(event))?;
        let mut fields = vec![
            ("rule".to_string(), rule.title.clone()),
            ("level".to_string(), rule.level.clone()),
        ];
        if let Some(t) = &rule.technique {
            fields.push(("attack.technique".to_string(), t.clone()));
        }
        Some(Signal {
            source: format!("sigma:{}", rule.title),
            severity: rule.severity(),
            fields,
            events: if event.id.is_empty() {
                Vec::new()
            } else {
                vec![event.id.clone()]
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULE: &str = r#"
title: Failed Password
level: high
tags: [attack.t1110]
detection:
  selection:
    message|contains: "Failed password"
  condition: selection
"#;

    fn event_with(message: &str) -> Event {
        let mut ev = Event::default();
        ev.set("message", message);
        ev
    }

    #[test]
    fn parses_and_matches() {
        let rule = SigmaRule::from_yaml(RULE).unwrap();
        assert_eq!(rule.title, "Failed Password");
        assert_eq!(rule.technique.as_deref(), Some("T1110"));
        assert!(rule.matches(&event_with("Failed password for root from 1.2.3.4")));
        assert!(!rule.matches(&event_with("Accepted password for alice")));
    }

    #[test]
    fn detector_emits_signal() {
        let det = SigmaDetector::new(vec![SigmaRule::from_yaml(RULE).unwrap()]);
        let signal = det.eval(&event_with("Failed password for root")).unwrap();
        assert_eq!(signal.source, "sigma:Failed Password");
        assert_eq!(signal.severity, 70);
        assert!(det.eval(&event_with("nothing to see")).is_none());
    }
}
