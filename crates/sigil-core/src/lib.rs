//! `sigil-core` — foundational types and plugin traits for **Sigil Search**.
//!
//! This crate is the stable contract shared by every other crate and by all
//! plugins: the normalized [`Event`] model (ECS-aligned by default) and the
//! plugin extension traits ([`Input`], [`Codec`], [`Schema`], [`Processor`],
//! [`Detector`], [`Output`], [`StorageBackend`], [`QueryFn`]).
//!
//! The downstream `sigil-siem` distribution builds on exactly these traits
//! (e.g. a Sigma [`Detector`], an OCSF [`Schema`]) without modifying the core.
//!
//! Status: **scaffold**. Types are placeholders so the workspace compiles; real
//! fields and implementations land per `docs/DESIGN.md` (§7 data model,
//! §11 plugin system).
//!
//! NOTE: production traits that do I/O are `async` (via `async-trait`). The
//! stubs below are synchronous to keep this crate dependency-free and offline.
#![allow(dead_code)]

use std::fmt;

/// Crate-local error placeholder.
#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for Error {}

/// Convenience result type used across the crate.
pub type Result<T> = std::result::Result<T, Error>;

// ---------------------------------------------------------------------------
// Domain model (DESIGN §7). Placeholders — to be fleshed out.
// ---------------------------------------------------------------------------

/// Unix epoch microseconds (placeholder representation).
pub type Timestamp = i64;

/// Canonical ECS field names used by the default schema (DESIGN §7).
///
/// Kept as plain constants so `sigil-core` stays dependency-free; downstream
/// crates and plugins reference these instead of hand-typing strings.
pub mod ecs {
    pub const TIMESTAMP: &str = "@timestamp";
    pub const MESSAGE: &str = "message";
    pub const HOST_NAME: &str = "host.name";
    pub const LOG_LEVEL: &str = "log.level";
    pub const LOG_SYSLOG_FACILITY: &str = "log.syslog.facility.code";
    pub const LOG_SYSLOG_SEVERITY: &str = "log.syslog.severity.code";
    pub const EVENT_DATASET: &str = "event.dataset";
    pub const EVENT_ORIGINAL: &str = "event.original";
    pub const PROCESS_NAME: &str = "process.name";
    pub const PROCESS_PID: &str = "process.pid";
}

/// A raw decoded record produced by a [`Codec`], before normalization.
#[derive(Debug, Clone, Default)]
pub struct Record {
    pub fields: Vec<(String, String)>,
}

impl Record {
    /// Build a record from any iterator of key/value pairs.
    pub fn from_pairs<I, K, V>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        Record {
            fields: pairs
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }

    /// First value for `key`, if present.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Append a field.
    pub fn push(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.fields.push((key.into(), value.into()));
    }
}

/// Normalized event (ECS-aligned by default; schema is pluggable). See DESIGN §7.
#[derive(Debug, Clone, Default)]
pub struct Event {
    pub id: String,
    pub ts: Timestamp,
    pub ingest_ts: Timestamp,
    /// ECS `data_stream.dataset`-style identifier of the source feed.
    pub dataset: String,
    pub tenant: String,
    pub fields: Vec<(String, String)>,
    pub template_id: Option<u64>,
    pub raw: Vec<u8>,
    /// Routing / detection tags (used by downstream plugins, e.g. SIEM).
    pub labels: Vec<String>,
}

impl Event {
    /// First value for a (typically ECS) field name, if present.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Set a field, replacing any existing value for the same key.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let key = key.into();
        let value = value.into();
        if let Some(slot) = self.fields.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
        } else {
            self.fields.push((key, value));
        }
    }
}

/// A signal/alert emitted by a [`Detector`] plugin.
///
/// This is the primary hook the SIEM distribution builds on (a Sigma rule is a
/// `Detector`; correlation consumes `Signal`s).
#[derive(Debug, Clone, Default)]
pub struct Signal {
    pub source: String,
    pub severity: u8,
    pub fields: Vec<(String, String)>,
    /// Ids of the events that produced this signal.
    pub events: Vec<String>,
}

/// Plugin manifest: identity + requested capabilities (DESIGN §11.2).
#[derive(Debug, Clone, Default)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub capabilities: Vec<String>,
}

// ---------------------------------------------------------------------------
// Plugin traits (DESIGN §11.1). Sync stubs today; async in real impls.
// ---------------------------------------------------------------------------

/// Common base every plugin implements.
pub trait Plugin {
    fn manifest(&self) -> &PluginManifest;
}

/// A source of raw events.
pub trait Input: Plugin {
    fn poll(&mut self) -> Result<Vec<Vec<u8>>>;
}

/// Decode raw bytes into [`Record`]s.
pub trait Codec: Plugin {
    fn decode(&self, raw: &[u8]) -> Result<Vec<Record>>;
}

/// Map a decoded record onto the normalized schema (ECS by default; OCSF in the
/// SIEM distribution).
pub trait Schema: Plugin {
    fn normalize(&self, record: Record) -> Result<Event>;
}

/// Map / filter / enrich a normalized event.
pub trait Processor: Plugin {
    fn process(&self, event: Event) -> Result<Vec<Event>>;
}

/// Stateless detection over a single event (the SIEM hook).
pub trait Detector: Plugin {
    fn eval(&self, event: &Event) -> Option<Signal>;
}

/// Emit events/signals to an external sink.
pub trait Output: Plugin {
    fn emit(&self, payload: &[u8]) -> Result<()>;
}

/// Pluggable storage backend for the indexer (or, e.g., a graph store).
pub trait StorageBackend: Plugin {
    fn flush(&self) -> Result<()>;
}

/// A user-defined function exposed to the query language.
pub trait QueryFn: Plugin {
    fn name(&self) -> &str;
}
