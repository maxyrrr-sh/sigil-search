//! `sigil-schema` — pluggable normalized schema, ECS by default (DESIGN §7).
//!
//! Phase 0 scope: [`EcsSchema`], the default [`Schema`] that maps the raw fields
//! emitted by the Phase 0 codecs (syslog, json) onto canonical ECS field names.
//! Unknown fields pass through unchanged (schema-on-read fallback). OCSF and
//! other schemas arrive as plugins in the SIEM distribution.
#![allow(dead_code)]

use sigil_core::{ecs, Event, Plugin, PluginManifest, Record, Result, Timestamp};

/// Default ECS-aligned schema. One instance per dataset (the source feed id).
pub struct EcsSchema {
    dataset: String,
    manifest: PluginManifest,
}

impl EcsSchema {
    /// Build an ECS schema that tags normalized events with `dataset`.
    pub fn new(dataset: impl Into<String>) -> Self {
        EcsSchema {
            dataset: dataset.into(),
            manifest: PluginManifest {
                name: "ecs".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                capabilities: vec!["normalize".to_string()],
            },
        }
    }
}

impl Plugin for EcsSchema {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
}

impl Schema for EcsSchema {
    fn normalize(&self, record: Record) -> Result<Event> {
        let now = now_micros();
        let mut event = Event {
            ts: now,
            ingest_ts: now,
            dataset: self.dataset.clone(),
            ..Default::default()
        };
        event.set(ecs::EVENT_DATASET, self.dataset.clone());

        for (key, value) in record.fields {
            match key.as_str() {
                "message" => event.set(ecs::MESSAGE, value),
                "host" => event.set(ecs::HOST_NAME, value),
                "app" => event.set(ecs::PROCESS_NAME, value),
                "pid" => event.set(ecs::PROCESS_PID, value),
                "facility" => event.set(ecs::LOG_SYSLOG_FACILITY, value),
                "severity" => {
                    if let Ok(code) = value.parse::<u8>() {
                        event.set(ecs::LOG_LEVEL, severity_label(code));
                    }
                    event.set(ecs::LOG_SYSLOG_SEVERITY, value);
                }
                "timestamp" => event.set(ecs::TIMESTAMP, value),
                // Already-ECS or unknown keys pass through unchanged.
                _ => event.set(key, value),
            }
        }
        Ok(event)
    }
}

// Re-export so callers can `use sigil_schema::Schema` alongside `EcsSchema`.
pub use sigil_core::Schema;

/// Map an RFC 5424 numeric severity to an ECS `log.level` label.
fn severity_label(code: u8) -> &'static str {
    match code {
        0 => "emergency",
        1 => "alert",
        2 => "critical",
        3 => "error",
        4 => "warning",
        5 => "notice",
        6 => "informational",
        _ => "debug",
    }
}

fn now_micros() -> Timestamp {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as Timestamp)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_syslog_fields_to_ecs() {
        let rec = Record::from_pairs([
            ("host", "web1"),
            ("app", "sshd"),
            ("severity", "3"),
            ("message", "boom"),
        ]);
        let ev = EcsSchema::new("syslog_main").normalize(rec).unwrap();
        assert_eq!(ev.dataset, "syslog_main");
        assert_eq!(ev.get(ecs::HOST_NAME), Some("web1"));
        assert_eq!(ev.get(ecs::PROCESS_NAME), Some("sshd"));
        assert_eq!(ev.get(ecs::LOG_LEVEL), Some("error"));
        assert_eq!(ev.get(ecs::MESSAGE), Some("boom"));
    }
}
