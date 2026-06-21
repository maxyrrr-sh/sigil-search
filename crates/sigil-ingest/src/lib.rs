//! `sigil-ingest` — inputs, codecs, template mining, processing pipeline (DESIGN §6).
//!
//! Phase 0 delivered the `json`/`syslog` codecs. Phase 1 adds the rest of the
//! codec set ([`KvCodec`], [`CsvCodec`], [`RegexCodec`], [`GrokCodec`],
//! [`CefCodec`]), Drain-style online [`template`] mining, and a configurable
//! processing [`pipeline`] (enrichment, masking, routing, dead-letter).
#![allow(dead_code)]

use sigil_core::PluginManifest;

pub mod codecs;
pub mod pipeline;
pub mod template;

pub use codecs::{
    CefCodec, CsvCodec, GrokCodec, JsonCodec, KvCodec, RegexCodec, SyslogCodec,
};
pub use pipeline::{Condition, Dest, GeoIp, Lookup, PiiMask, Pipeline, PipelineOutcome};
pub use template::TemplateMiner;

/// Build a plugin manifest for an ingest component.
pub(crate) fn manifest(name: &str, capability: &str) -> PluginManifest {
    PluginManifest {
        name: name.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        capabilities: vec![capability.to_string()],
    }
}

/// Look up a configuration-free codec by name (`json`, `syslog`, `kv`, `cef`).
/// Codecs that need parameters (`csv`, `regex`, `grok`) are constructed directly.
pub fn default_codec(kind: &str) -> Option<Box<dyn sigil_core::Codec + Send + Sync>> {
    match kind {
        "json" => Some(Box::new(JsonCodec::default())),
        "syslog" => Some(Box::new(SyslogCodec::default())),
        "kv" => Some(Box::new(KvCodec::default())),
        "cef" => Some(Box::new(CefCodec::default())),
        _ => None,
    }
}
