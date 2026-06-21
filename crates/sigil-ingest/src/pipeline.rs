//! Processing pipeline (DESIGN §6): an ordered set of conditional steps over
//! normalized [`Event`]s, followed by routing to a destination. Failed steps
//! send the event to the dead-letter destination instead of dropping it.

use std::collections::HashMap;
use std::net::Ipv4Addr;

use regex::Regex;
use sigil_core::{Event, Plugin, PluginManifest, Processor, Result};

use crate::manifest;

/// Where a processed event is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dest {
    Index,
    DeadLetter,
    Drop,
}

/// A predicate over an event's fields. Phase 1 supports single comparisons;
/// boolean combinators can be added without changing callers.
#[derive(Debug, Clone)]
pub enum Condition {
    Always,
    Exists(String),
    Eq(String, String),
    Ne(String, String),
    Contains(String, String),
}

impl Condition {
    /// Parse `*`, `exists F`, `F == V`, `F != V`, or `F contains V`.
    pub fn parse(s: &str) -> Result<Condition> {
        let s = s.trim();
        if s.is_empty() || s == "*" {
            return Ok(Condition::Always);
        }
        if let Some(field) = s.strip_prefix("exists ") {
            return Ok(Condition::Exists(field.trim().to_string()));
        }
        for (op, ctor) in [
            ("==", Condition::Eq as fn(String, String) -> Condition),
            ("!=", Condition::Ne as fn(String, String) -> Condition),
            (" contains ", Condition::Contains as fn(String, String) -> Condition),
        ] {
            if let Some(idx) = s.find(op) {
                let field = s[..idx].trim().to_string();
                let value = unquote(s[idx + op.len()..].trim());
                return Ok(ctor(field, value));
            }
        }
        Err(sigil_core::Error(format!("unparsable condition: {s}")))
    }

    pub fn eval(&self, event: &Event) -> bool {
        match self {
            Condition::Always => true,
            Condition::Exists(f) => event.get(f).is_some(),
            Condition::Eq(f, v) => event.get(f) == Some(v.as_str()),
            Condition::Ne(f, v) => event.get(f) != Some(v.as_str()),
            Condition::Contains(f, v) => event.get(f).is_some_and(|x| x.contains(v.as_str())),
        }
    }
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
        || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

enum Action {
    Process(Box<dyn Processor + Send + Sync>),
    Set { field: String, value: String },
    Drop,
}

struct Step {
    when: Condition,
    action: Action,
}

struct RouteRule {
    when: Condition,
    to: Dest,
}

/// The outcome of running one event through the pipeline: zero or more events,
/// each tagged with the destination it should be sent to.
#[derive(Debug, Default)]
pub struct PipelineOutcome {
    pub routed: Vec<(Event, Dest)>,
}

/// A configurable processing pipeline.
#[derive(Default)]
pub struct Pipeline {
    steps: Vec<Step>,
    routes: Vec<RouteRule>,
}

impl Pipeline {
    pub fn new() -> Self {
        Pipeline::default()
    }

    /// Add a processor step, optionally gated by a condition.
    pub fn with_processor(
        mut self,
        when: Condition,
        processor: Box<dyn Processor + Send + Sync>,
    ) -> Self {
        self.steps.push(Step {
            when,
            action: Action::Process(processor),
        });
        self
    }

    /// Add a `set field = value` step.
    pub fn with_set(mut self, when: Condition, field: impl Into<String>, value: impl Into<String>) -> Self {
        self.steps.push(Step {
            when,
            action: Action::Set {
                field: field.into(),
                value: value.into(),
            },
        });
        self
    }

    /// Add a drop step (events matching `when` are discarded).
    pub fn with_drop(mut self, when: Condition) -> Self {
        self.steps.push(Step {
            when,
            action: Action::Drop,
        });
        self
    }

    /// Add a routing rule. Rules are evaluated in order; first match wins.
    pub fn with_route(mut self, when: Condition, to: Dest) -> Self {
        self.routes.push(RouteRule { when, to });
        self
    }

    /// Run a single event through the pipeline.
    pub fn run(&self, event: Event) -> PipelineOutcome {
        let mut live = vec![event];
        let mut dead = Vec::new();

        for step in &self.steps {
            let mut next = Vec::with_capacity(live.len());
            for ev in live {
                if !step.when.eval(&ev) {
                    next.push(ev);
                    continue;
                }
                match &step.action {
                    Action::Drop => {}
                    Action::Set { field, value } => {
                        let mut ev = ev;
                        ev.set(field.clone(), value.clone());
                        next.push(ev);
                    }
                    Action::Process(p) => {
                        let backup = ev.clone();
                        match p.process(ev) {
                            Ok(out) => next.extend(out),
                            Err(_) => dead.push(backup),
                        }
                    }
                }
            }
            live = next;
        }

        let mut outcome = PipelineOutcome::default();
        for ev in live {
            let dest = self
                .routes
                .iter()
                .find(|r| r.when.eval(&ev))
                .map(|r| r.to)
                .unwrap_or(Dest::Index);
            outcome.routed.push((ev, dest));
        }
        for ev in dead {
            outcome.routed.push((ev, Dest::DeadLetter));
        }
        outcome
    }
}

// ---------------------------------------------------------------------------
// Enrichment processors
// ---------------------------------------------------------------------------

/// DB-free IP enrichment: classifies an address as loopback/private/public and
/// optionally maps known prefixes to a country (from a config-provided table).
/// Real MaxMind/GeoIP2 lookups arrive as a plugin; this keeps the core honest.
pub struct GeoIp {
    source_fields: Vec<String>,
    prefix_country: Vec<(String, String)>,
    manifest: PluginManifest,
}

impl GeoIp {
    pub fn new(source_fields: Vec<String>, prefix_country: Vec<(String, String)>) -> Self {
        GeoIp {
            source_fields,
            prefix_country,
            manifest: manifest("geoip", "process"),
        }
    }

    pub fn default_fields() -> Self {
        GeoIp::new(
            vec![
                "source.ip".into(),
                "client.ip".into(),
                "host.ip".into(),
            ],
            Vec::new(),
        )
    }
}

impl Plugin for GeoIp {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
}

impl Processor for GeoIp {
    fn process(&self, mut event: Event) -> Result<Vec<Event>> {
        for field in &self.source_fields {
            let Some(ip_str) = event.get(field).map(str::to_string) else {
                continue;
            };
            let Ok(ip) = ip_str.parse::<Ipv4Addr>() else {
                continue;
            };
            let prefix = field.trim_end_matches(".ip");
            let scope = if ip.is_loopback() {
                "loopback"
            } else if ip.is_private() || ip.is_link_local() {
                "private"
            } else {
                "public"
            };
            event.set(format!("{prefix}.geo.scope"), scope);
            for (pfx, country) in &self.prefix_country {
                if ip_str.starts_with(pfx) {
                    event.set(format!("{prefix}.geo.country_iso_code"), country.clone());
                    break;
                }
            }
        }
        Ok(vec![event])
    }
}

/// Asset/identity enrichment: look up a key field in a table and copy the
/// associated fields (prefixed) onto the event.
pub struct Lookup {
    key_field: String,
    target_prefix: String,
    table: HashMap<String, Vec<(String, String)>>,
    manifest: PluginManifest,
}

impl Lookup {
    pub fn new(
        key_field: impl Into<String>,
        target_prefix: impl Into<String>,
        table: HashMap<String, Vec<(String, String)>>,
    ) -> Self {
        Lookup {
            key_field: key_field.into(),
            target_prefix: target_prefix.into(),
            table,
            manifest: manifest("lookup", "process"),
        }
    }
}

impl Plugin for Lookup {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
}

impl Processor for Lookup {
    fn process(&self, mut event: Event) -> Result<Vec<Event>> {
        if let Some(key) = event.get(&self.key_field).map(str::to_string) {
            if let Some(attrs) = self.table.get(&key) {
                for (k, v) in attrs {
                    event.set(format!("{}.{k}", self.target_prefix), v.clone());
                }
            }
        }
        Ok(vec![event])
    }
}

// ---------------------------------------------------------------------------
// PII masking
// ---------------------------------------------------------------------------

/// Redact values matching sensitive patterns. Operates on the named fields, or
/// on all fields when `fields` is empty.
pub struct PiiMask {
    fields: Vec<String>,
    patterns: Vec<Regex>,
    replacement: String,
    manifest: PluginManifest,
}

impl PiiMask {
    /// Built-in masks for emails and IPv4 addresses.
    pub fn builtin(fields: Vec<String>) -> Self {
        let patterns = vec![
            Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}").unwrap(),
            Regex::new(r"\b\d{1,3}(?:\.\d{1,3}){3}\b").unwrap(),
        ];
        PiiMask {
            fields,
            patterns,
            replacement: "***".to_string(),
            manifest: manifest("pii-mask", "process"),
        }
    }

    pub fn with_pattern(mut self, pattern: &str) -> Result<Self> {
        self.patterns
            .push(Regex::new(pattern).map_err(|e| sigil_core::Error(format!("mask: {e}")))?);
        Ok(self)
    }

    fn mask_value(&self, value: &str) -> String {
        let mut out = value.to_string();
        for re in &self.patterns {
            out = re.replace_all(&out, self.replacement.as_str()).into_owned();
        }
        out
    }
}

impl Plugin for PiiMask {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
}

impl Processor for PiiMask {
    fn process(&self, mut event: Event) -> Result<Vec<Event>> {
        for (key, value) in event.fields.iter_mut() {
            if self.fields.is_empty() || self.fields.iter().any(|f| f == key) {
                *value = self.mask_value(value);
            }
        }
        Ok(vec![event])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sigil_core::ecs;

    fn ev_with(field: &str, value: &str) -> Event {
        let mut ev = Event::default();
        ev.set(field, value);
        ev
    }

    #[test]
    fn condition_parsing_and_eval() {
        let mut ev = Event::default();
        ev.set(ecs::LOG_LEVEL, "error");
        assert!(Condition::parse("log.level == error").unwrap().eval(&ev));
        assert!(Condition::parse("log.level != info").unwrap().eval(&ev));
        assert!(Condition::parse("exists log.level").unwrap().eval(&ev));
        assert!(!Condition::parse("exists host.name").unwrap().eval(&ev));
        assert!(Condition::parse("*").unwrap().eval(&ev));
    }

    #[test]
    fn geoip_classifies_scope() {
        let out = GeoIp::default_fields()
            .process(ev_with("source.ip", "10.0.0.5"))
            .unwrap();
        assert_eq!(out[0].get("source.geo.scope"), Some("private"));
        let out = GeoIp::default_fields()
            .process(ev_with("source.ip", "8.8.8.8"))
            .unwrap();
        assert_eq!(out[0].get("source.geo.scope"), Some("public"));
    }

    #[test]
    fn pii_mask_redacts() {
        let out = PiiMask::builtin(Vec::new())
            .process(ev_with(ecs::MESSAGE, "mail alice@example.com from 10.0.0.9"))
            .unwrap();
        let msg = out[0].get(ecs::MESSAGE).unwrap();
        assert!(!msg.contains("alice@example.com"));
        assert!(!msg.contains("10.0.0.9"));
        assert!(msg.contains("***"));
    }

    #[test]
    fn routing_and_drop() {
        let pipe = Pipeline::new()
            .with_drop(Condition::parse("log.level == debug").unwrap())
            .with_route(Condition::parse("log.level == error").unwrap(), Dest::DeadLetter);

        let mut err = Event::default();
        err.set(ecs::LOG_LEVEL, "error");
        let out = pipe.run(err);
        assert_eq!(out.routed.len(), 1);
        assert_eq!(out.routed[0].1, Dest::DeadLetter);

        let mut dbg = Event::default();
        dbg.set(ecs::LOG_LEVEL, "debug");
        assert!(pipe.run(dbg).routed.is_empty(), "debug should be dropped");

        let mut info = Event::default();
        info.set(ecs::LOG_LEVEL, "info");
        assert_eq!(pipe.run(info).routed[0].1, Dest::Index);
    }
}
