//! An example first-party [`Detector`] plugin.
//!
//! Deliberately **generic** (a substring match over any field) so it stays in
//! the platform as a demonstration of the detect extension point — domain
//! detectors (Sigma, etc.) live in the SIEM distribution, not here.

use sigil_core::{Detector, Event, Plugin, PluginManifest, Signal};

/// Emits a [`Signal`] when `field` contains `needle`.
pub struct KeywordDetector {
    source: String,
    field: String,
    needle: String,
    severity: u8,
    manifest: PluginManifest,
}

impl KeywordDetector {
    pub fn new(
        source: impl Into<String>,
        field: impl Into<String>,
        needle: impl Into<String>,
        severity: u8,
    ) -> Self {
        let source = source.into();
        let field = field.into();
        let needle = needle.into();
        let manifest = PluginManifest {
            name: source.clone(),
            version: "0.1.0".to_string(),
            capabilities: vec![format!("read:field:{field}"), "emit:signal".to_string()],
        };
        KeywordDetector {
            source,
            field,
            needle,
            severity,
            manifest,
        }
    }
}

impl Plugin for KeywordDetector {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
}

impl Detector for KeywordDetector {
    fn eval(&self, event: &Event) -> Option<Signal> {
        let value = event.get(&self.field)?;
        if !value.contains(&self.needle) {
            return None;
        }
        Some(Signal {
            source: self.source.clone(),
            severity: self.severity,
            fields: vec![
                ("rule".to_string(), self.source.clone()),
                ("matched_field".to_string(), self.field.clone()),
                ("needle".to_string(), self.needle.clone()),
            ],
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
    use sigil_core::ecs;

    #[test]
    fn fires_only_on_match() {
        let det = KeywordDetector::new("error-level", ecs::LOG_LEVEL, "error", 50);
        let mut hit = Event::default();
        hit.set(ecs::LOG_LEVEL, "error");
        let mut miss = Event::default();
        miss.set(ecs::LOG_LEVEL, "info");

        let sig = det.eval(&hit).expect("should fire");
        assert_eq!(sig.source, "error-level");
        assert_eq!(sig.severity, 50);
        assert!(det.eval(&miss).is_none());
    }
}
