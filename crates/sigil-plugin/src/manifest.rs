//! Manifest validation + content digest (DESIGN §11.2).
//!
//! Cryptographic signing (ed25519 / minisign) is deferred; [`digest`] provides a
//! stable, non-cryptographic content fingerprint to pin a manifest against in
//! the meantime, and [`verify_digest`] checks it.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use sigil_core::PluginManifest;

/// Validate the basic shape of a manifest.
pub fn validate(m: &PluginManifest) -> anyhow::Result<()> {
    if m.name.trim().is_empty() {
        anyhow::bail!("plugin manifest has no name");
    }
    if m.version.trim().is_empty() {
        anyhow::bail!("plugin '{}' manifest has no version", m.name);
    }
    Ok(())
}

/// A stable content fingerprint over name + version + (sorted) capabilities.
pub fn digest(m: &PluginManifest) -> String {
    let mut caps = m.capabilities.clone();
    caps.sort();
    let canonical = format!("{}|{}|{}", m.name, m.version, caps.join(","));
    let mut h = DefaultHasher::new();
    canonical.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Check a manifest against a pinned digest.
pub fn verify_digest(m: &PluginManifest, expected: &str) -> bool {
    digest(m) == expected
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(caps: &[&str]) -> PluginManifest {
        PluginManifest {
            name: "demo".into(),
            version: "1.0.0".into(),
            capabilities: caps.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn digest_is_order_independent_and_verifies() {
        let a = manifest(&["read:field:message", "emit:signal"]);
        let b = manifest(&["emit:signal", "read:field:message"]);
        assert_eq!(digest(&a), digest(&b));
        assert!(verify_digest(&a, &digest(&b)));
        assert!(!verify_digest(&a, "deadbeefdeadbeef"));
    }

    #[test]
    fn validation_rejects_empty() {
        assert!(validate(&manifest(&[])).is_ok());
        let mut bad = manifest(&[]);
        bad.name = String::new();
        assert!(validate(&bad).is_err());
    }
}
