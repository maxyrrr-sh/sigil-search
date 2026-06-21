//! Capability-based permissions (DESIGN §11.2).
//!
//! A plugin's [`PluginManifest`](sigil_core::PluginManifest) declares the
//! capabilities it *requests*; the host *grants* a set (operator policy). A
//! plugin may only be registered if every requested capability is granted.

/// A single capability a plugin may request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capability {
    /// Read a field (`read:field:NAME`, or `read:field:*` for any).
    ReadField(String),
    /// Write a field (`write:field:NAME`, or `*`).
    WriteField(String),
    /// Emit detection signals (`emit:signal`).
    EmitSignal,
    /// Open network connections (`network`).
    Network,
    /// Any other, opaque capability token.
    Other(String),
}

impl Capability {
    pub fn parse(s: &str) -> Capability {
        let s = s.trim();
        if let Some(rest) = s.strip_prefix("read:field:") {
            return Capability::ReadField(rest.to_string());
        }
        if let Some(rest) = s.strip_prefix("write:field:") {
            return Capability::WriteField(rest.to_string());
        }
        match s {
            "emit:signal" => Capability::EmitSignal,
            "network" | "network:connect" => Capability::Network,
            other => Capability::Other(other.to_string()),
        }
    }

    pub fn token(&self) -> String {
        match self {
            Capability::ReadField(f) => format!("read:field:{f}"),
            Capability::WriteField(f) => format!("write:field:{f}"),
            Capability::EmitSignal => "emit:signal".to_string(),
            Capability::Network => "network".to_string(),
            Capability::Other(s) => s.clone(),
        }
    }

    /// Does `self` (a granted capability) cover `requested`?
    fn grants(&self, requested: &Capability) -> bool {
        match (self, requested) {
            (Capability::ReadField(g), Capability::ReadField(r)) => g == "*" || g == r,
            (Capability::WriteField(g), Capability::WriteField(r)) => g == "*" || g == r,
            (Capability::EmitSignal, Capability::EmitSignal) => true,
            (Capability::Network, Capability::Network) => true,
            (Capability::Other(g), Capability::Other(r)) => g == r,
            _ => false,
        }
    }
}

/// A set of capabilities (granted by the host, or requested by a plugin).
#[derive(Debug, Clone, Default)]
pub struct CapabilitySet {
    caps: Vec<Capability>,
}

impl CapabilitySet {
    pub fn new(caps: Vec<Capability>) -> Self {
        CapabilitySet { caps }
    }

    /// Parse capability tokens (e.g. from a manifest or config).
    pub fn parse(tokens: &[String]) -> Self {
        CapabilitySet {
            caps: tokens.iter().map(|t| Capability::parse(t)).collect(),
        }
    }

    /// A safe default grant: read/write any field + emit signals, but **no
    /// network**. Operators widen this explicitly.
    pub fn safe_default() -> Self {
        CapabilitySet::new(vec![
            Capability::ReadField("*".to_string()),
            Capability::WriteField("*".to_string()),
            Capability::EmitSignal,
        ])
    }

    pub fn requested(&self) -> &[Capability] {
        &self.caps
    }

    pub fn allows(&self, requested: &Capability) -> bool {
        self.caps.iter().any(|g| g.grants(requested))
    }

    /// Returns the first requested capability not covered by this set, if any.
    pub fn missing(&self, requested: &[Capability]) -> Option<Capability> {
        requested.iter().find(|r| !self.allows(r)).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards_and_denials() {
        let granted = CapabilitySet::safe_default();
        assert!(granted.allows(&Capability::parse("read:field:message")));
        assert!(granted.allows(&Capability::parse("write:field:host.name")));
        assert!(granted.allows(&Capability::EmitSignal));
        // Network is not in the safe default.
        assert!(!granted.allows(&Capability::Network));
    }

    #[test]
    fn missing_reports_first_ungranted() {
        let granted = CapabilitySet::new(vec![Capability::ReadField("message".into())]);
        let requested = CapabilitySet::parse(&[
            "read:field:message".into(),
            "network".into(),
        ]);
        assert_eq!(
            granted.missing(requested.requested()),
            Some(Capability::Network)
        );
    }
}
