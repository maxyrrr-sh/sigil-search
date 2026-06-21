//! `siem-correlate` — signal correlation + a provenance graph backend.
//!
//! Demonstrates two things the platform's plugin API must support for a SIEM:
//!  * a **correlator** that consumes [`Signal`]s and escalates when a source
//!    repeats within a window (e.g. brute force), and
//!  * a **graph store** implementing the platform's `sigil_core::StorageBackend`
//!    extension trait, recording signal→event provenance edges.
//!
//! Both are built only on `sigil-core` types — no core change required.

use std::collections::HashMap;
use std::sync::Mutex;

use sigil_core::{Plugin, PluginManifest, Result, Signal, StorageBackend};

/// An escalated, correlated finding (e.g. N detections from one source).
#[derive(Debug, Clone)]
pub struct Correlation {
    pub source: String,
    pub count: usize,
    pub severity: u8,
    pub events: Vec<String>,
}

/// Counts signals per source and escalates at a threshold.
pub struct Correlator {
    threshold: usize,
    state: Mutex<HashMap<String, Aggregate>>,
}

#[derive(Default)]
struct Aggregate {
    count: usize,
    max_severity: u8,
    events: Vec<String>,
    fired: bool,
}

impl Correlator {
    pub fn new(threshold: usize) -> Self {
        Correlator {
            threshold: threshold.max(1),
            state: Mutex::new(HashMap::new()),
        }
    }

    /// Observe a signal; returns a [`Correlation`] the first time a source
    /// reaches the threshold.
    pub fn observe(&self, signal: &Signal) -> Option<Correlation> {
        let mut state = self.state.lock().expect("correlator poisoned");
        let agg = state.entry(signal.source.clone()).or_default();
        agg.count += 1;
        agg.max_severity = agg.max_severity.max(signal.severity);
        agg.events.extend(signal.events.iter().cloned());
        if agg.count >= self.threshold && !agg.fired {
            agg.fired = true;
            // Correlated findings are escalated above the per-signal severity.
            Some(Correlation {
                source: signal.source.clone(),
                count: agg.count,
                severity: agg.max_severity.saturating_add(10).min(100),
                events: agg.events.clone(),
            })
        } else {
            None
        }
    }
}

/// A provenance graph: nodes are sources/events, edges link a source to the
/// events that triggered it. Implements the platform's [`StorageBackend`].
pub struct GraphBackend {
    edges: Mutex<Vec<(String, String)>>,
    manifest: PluginManifest,
}

impl Default for GraphBackend {
    fn default() -> Self {
        GraphBackend {
            edges: Mutex::new(Vec::new()),
            manifest: PluginManifest {
                name: "provenance-graph".to_string(),
                version: "0.1.0".to_string(),
                capabilities: vec!["write:field:*".to_string()],
            },
        }
    }
}

impl GraphBackend {
    /// Record `source -> event` edges for a signal.
    pub fn record(&self, signal: &Signal) {
        let mut edges = self.edges.lock().expect("graph poisoned");
        for ev in &signal.events {
            edges.push((signal.source.clone(), ev.clone()));
        }
        if signal.events.is_empty() {
            edges.push((signal.source.clone(), "<unattributed>".to_string()));
        }
    }

    pub fn edge_count(&self) -> usize {
        self.edges.lock().expect("graph poisoned").len()
    }

    pub fn node_count(&self) -> usize {
        let edges = self.edges.lock().expect("graph poisoned");
        let mut nodes = std::collections::HashSet::new();
        for (a, b) in edges.iter() {
            nodes.insert(a.clone());
            nodes.insert(b.clone());
        }
        nodes.len()
    }
}

impl Plugin for GraphBackend {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
}

impl StorageBackend for GraphBackend {
    fn flush(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signal(source: &str, sev: u8, event: &str) -> Signal {
        Signal {
            source: source.into(),
            severity: sev,
            fields: vec![],
            events: vec![event.into()],
        }
    }

    #[test]
    fn escalates_at_threshold_once() {
        let c = Correlator::new(3);
        assert!(c.observe(&signal("sigma:Brute", 70, "e1")).is_none());
        assert!(c.observe(&signal("sigma:Brute", 70, "e2")).is_none());
        let corr = c.observe(&signal("sigma:Brute", 70, "e3")).expect("escalates");
        assert_eq!(corr.count, 3);
        assert_eq!(corr.severity, 80);
        assert_eq!(corr.events, vec!["e1", "e2", "e3"]);
        // Does not re-fire.
        assert!(c.observe(&signal("sigma:Brute", 70, "e4")).is_none());
    }

    #[test]
    fn graph_records_edges() {
        let g = GraphBackend::default();
        g.record(&signal("sigma:Brute", 70, "e1"));
        g.record(&signal("sigma:Brute", 70, "e2"));
        assert_eq!(g.edge_count(), 2);
        // nodes: sigma:Brute, e1, e2
        assert_eq!(g.node_count(), 3);
        assert!(g.flush().is_ok());
    }
}
