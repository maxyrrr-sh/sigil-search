//! `siem-alert` — an alerting [`Output`] plugin.
//!
//! Emits alerts to an append-only NDJSON file (a stand-in for Slack / PagerDuty
//! / email). It requests the `network` capability — which the platform's
//! *safe-default* grant denies — so the SIEM distribution must explicitly grant
//! network to load it. That is the capability model doing its job.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use sigil_core::{Output, Plugin, PluginManifest, Result, Signal};

/// Writes alert payloads to a file sink.
pub struct AlertOutput {
    path: String,
    count: AtomicU64,
    file: Mutex<Option<std::fs::File>>,
    manifest: PluginManifest,
}

impl AlertOutput {
    pub fn new(path: impl Into<String>) -> Self {
        AlertOutput {
            path: path.into(),
            count: AtomicU64::new(0),
            file: Mutex::new(None),
            manifest: PluginManifest {
                name: "alert".to_string(),
                version: "0.1.0".to_string(),
                capabilities: vec!["network".to_string()],
            },
        }
    }

    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Build an alert document from a detection signal and emit it.
    pub fn alert_from_signal(&self, signal: &Signal, kind: &str) -> Result<()> {
        let mut fields = serde_json::Map::new();
        for (k, v) in &signal.fields {
            fields.insert(k.clone(), v.clone().into());
        }
        let mut obj = serde_json::Map::new();
        obj.insert("kind".into(), kind.into());
        obj.insert("source".into(), signal.source.clone().into());
        obj.insert("severity".into(), signal.severity.into());
        obj.insert("events".into(), signal.events.clone().into());
        obj.insert("fields".into(), serde_json::Value::Object(fields));
        self.emit(serde_json::Value::Object(obj).to_string().as_bytes())
    }
}

impl Plugin for AlertOutput {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
}

impl Output for AlertOutput {
    fn emit(&self, payload: &[u8]) -> Result<()> {
        use std::io::Write;
        self.count.fetch_add(1, Ordering::Relaxed);
        let mut guard = self.file.lock().expect("alert sink poisoned");
        if guard.is_none() {
            *guard = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .ok();
        }
        if let Some(f) = guard.as_mut() {
            f.write_all(payload)
                .and_then(|_| f.write_all(b"\n"))
                .map_err(|e| sigil_core::Error(format!("alert write: {e}")))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_and_counts() {
        let path = std::env::temp_dir().join(format!("siem-alert-{}.ndjson", std::process::id()));
        let out = AlertOutput::new(path.to_string_lossy().to_string());
        let signal = Signal {
            source: "sigma:Test".into(),
            severity: 70,
            fields: vec![("rule".into(), "Test".into())],
            events: vec![],
        };
        out.alert_from_signal(&signal, "detection").unwrap();
        assert_eq!(out.count(), 1);
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("sigma:Test"));
        std::fs::remove_file(&path).ok();
    }
}
