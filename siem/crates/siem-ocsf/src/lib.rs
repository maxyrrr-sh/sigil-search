//! `siem-ocsf` — an OCSF normalization [`Schema`] plugin.
//!
//! Proves that the SIEM's normalized schema (OCSF) is just a [`Schema`] plugin
//! on the platform: it implements the exact `sigil_core::Schema` trait the ECS
//! schema does, so the core needs no change to speak OCSF.

use regex::Regex;
use sigil_core::{Event, Plugin, PluginManifest, Record, Result, Schema, Timestamp};

/// Maps decoded records onto OCSF (Open Cybersecurity Schema Framework) fields.
pub struct OcsfSchema {
    ipv4: Regex,
    manifest: PluginManifest,
}

impl Default for OcsfSchema {
    fn default() -> Self {
        OcsfSchema {
            ipv4: Regex::new(r"\b\d{1,3}(?:\.\d{1,3}){3}\b").expect("static ipv4 regex"),
            manifest: PluginManifest {
                name: "ocsf".to_string(),
                version: "1.1.0".to_string(),
                capabilities: vec!["normalize".to_string()],
            },
        }
    }
}

impl Plugin for OcsfSchema {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
}

impl Schema for OcsfSchema {
    fn normalize(&self, record: Record) -> Result<Event> {
        let now = now_micros();
        let mut event = Event {
            ts: now,
            ingest_ts: now,
            dataset: "ocsf".to_string(),
            ..Default::default()
        };
        event.set("metadata.version", "1.1.0");
        event.set("metadata.product.name", "sigil-siem");

        let message = record.get("message").unwrap_or_default().to_string();

        for (key, value) in &record.fields {
            match key.as_str() {
                "host" => event.set("device.hostname", value.clone()),
                "app" => event.set("actor.process.name", value.clone()),
                "pid" => event.set("actor.process.pid", value.clone()),
                "user" | "username" => event.set("actor.user.name", value.clone()),
                "src" | "source.ip" | "client.ip" => event.set("src_endpoint.ip", value.clone()),
                "dst" | "dest.ip" => event.set("dst_endpoint.ip", value.clone()),
                "message" => event.set("message", value.clone()),
                "severity" => {
                    if let Ok(code) = value.parse::<u8>() {
                        event.set("severity_id", ocsf_severity(code).to_string());
                    }
                }
                // Unknown source fields are retained under an `unmapped.` prefix.
                other => event.set(format!("unmapped.{other}"), value.clone()),
            }
        }

        // Best-effort: pull a source IP out of the message if not already set.
        if event.get("src_endpoint.ip").is_none() {
            if let Some(m) = self.ipv4.find(&message) {
                event.set("src_endpoint.ip", m.as_str().to_string());
            }
        }

        // Classify the OCSF event class from message content.
        let (class_uid, class_name, category_uid) = classify(&message);
        event.set("class_uid", class_uid.to_string());
        event.set("class_name", class_name);
        event.set("category_uid", category_uid.to_string());

        Ok(event)
    }
}

/// Map a syslog severity (0..7) to an OCSF `severity_id` (1..6).
fn ocsf_severity(syslog: u8) -> u8 {
    match syslog {
        0 | 1 => 6,     // Fatal
        2 | 3 => 5,     // Critical
        4 => 4,         // High
        5 => 3,         // Medium
        6 => 2,         // Low
        _ => 1,         // Informational
    }
}

/// Pick an OCSF class from message keywords (a small, illustrative mapping).
fn classify(message: &str) -> (u32, &'static str, u32) {
    let m = message.to_ascii_lowercase();
    if m.contains("password") || m.contains("login") || m.contains("authentication") || m.contains("session opened") {
        (3002, "Authentication", 3) // category 3 = IAM
    } else if m.contains("curl ") || m.contains("wget ") || m.contains("/bin/") || m.contains("exec") {
        (1007, "Process Activity", 1) // category 1 = System Activity
    } else if m.contains("get ") || m.contains("post ") || m.contains("http") {
        (4002, "HTTP Activity", 4) // category 4 = Network Activity
    } else {
        (1001, "File System Activity", 1)
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
    fn maps_auth_event_to_ocsf() {
        let rec = Record::from_pairs([
            ("host", "web01"),
            ("app", "sshd"),
            ("message", "Failed password for root from 203.0.113.7 port 52344"),
        ]);
        let ev = OcsfSchema::default().normalize(rec).unwrap();
        assert_eq!(ev.get("class_name"), Some("Authentication"));
        assert_eq!(ev.get("class_uid"), Some("3002"));
        assert_eq!(ev.get("device.hostname"), Some("web01"));
        assert_eq!(ev.get("actor.process.name"), Some("sshd"));
        // IP extracted from the message.
        assert_eq!(ev.get("src_endpoint.ip"), Some("203.0.113.7"));
    }
}
