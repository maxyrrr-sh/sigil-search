//! The plugin host: a typed registry of compile-time plugins plus the lifecycle
//! for configuration-declared plugins (DESIGN §11.1, §11.4).
//!
//! Compile-time (first-party) plugins are registered as trait objects, gated by
//! capability + manifest checks. Config-declared `wasm`/`grpc` plugins are
//! validated here and reported as **pending** — their runtimes (wasmtime
//! Component Model, gRPC/Arrow Flight sidecars) are deferred.

use std::collections::HashMap;
use std::sync::Arc;

use sigil_core::{Codec, Detector, Output, PluginManifest, Processor, Schema};

use crate::capability::CapabilitySet;
use crate::version::ApiVersion;

/// Outcome of attempting to load a declared plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginState {
    /// Loaded and available.
    Registered,
    /// Valid but its runtime is not implemented yet (wasm/grpc).
    Pending(String),
    /// Refused (bad version, ungranted capability, unknown kind/name).
    Rejected(String),
}

/// The lifecycle result for one declared plugin.
#[derive(Debug, Clone)]
pub struct PluginStatus {
    pub name: String,
    pub kind: String,
    pub state: PluginState,
}

impl PluginStatus {
    pub fn summary(&self) -> String {
        match &self.state {
            PluginState::Registered => format!("{} ({}) registered", self.name, self.kind),
            PluginState::Pending(why) => format!("{} ({}) pending: {why}", self.name, self.kind),
            PluginState::Rejected(why) => format!("{} ({}) rejected: {why}", self.name, self.kind),
        }
    }
}

/// A plugin declared in configuration.
#[derive(Debug, Clone)]
pub struct PluginSpec {
    pub name: String,
    pub kind: String,
    pub path: Option<String>,
    pub capabilities: Vec<String>,
    pub api: Option<String>,
}

#[derive(Default)]
struct Registry {
    codecs: HashMap<String, Arc<dyn Codec + Send + Sync>>,
    processors: HashMap<String, Arc<dyn Processor + Send + Sync>>,
    detectors: HashMap<String, Arc<dyn Detector + Send + Sync>>,
    schemas: HashMap<String, Arc<dyn Schema + Send + Sync>>,
    outputs: HashMap<String, Arc<dyn Output + Send + Sync>>,
}

/// Registers and resolves plugins, enforcing capability + version policy.
pub struct PluginHost {
    api_version: ApiVersion,
    granted: CapabilitySet,
    registry: Registry,
}

impl PluginHost {
    pub fn new(granted: CapabilitySet) -> Self {
        PluginHost {
            api_version: ApiVersion::CURRENT,
            granted,
            registry: Registry::default(),
        }
    }

    /// A host with the safe-default capability grant (read/write fields + emit
    /// signals, no network).
    pub fn with_safe_defaults() -> Self {
        PluginHost::new(CapabilitySet::safe_default())
    }

    pub fn api_version(&self) -> ApiVersion {
        self.api_version
    }

    fn authorize(&self, m: &PluginManifest) -> anyhow::Result<()> {
        crate::manifest::validate(m)?;
        let requested = CapabilitySet::parse(&m.capabilities);
        if let Some(missing) = self.granted.missing(requested.requested()) {
            anyhow::bail!(
                "plugin '{}' requests ungranted capability '{}'",
                m.name,
                missing.token()
            );
        }
        Ok(())
    }

    // --- compile-time registration --------------------------------------

    pub fn register_codec(&mut self, plugin: Arc<dyn Codec + Send + Sync>) -> anyhow::Result<()> {
        self.authorize(plugin.manifest())?;
        self.registry
            .codecs
            .insert(plugin.manifest().name.clone(), plugin);
        Ok(())
    }

    pub fn register_processor(
        &mut self,
        plugin: Arc<dyn Processor + Send + Sync>,
    ) -> anyhow::Result<()> {
        self.authorize(plugin.manifest())?;
        self.registry
            .processors
            .insert(plugin.manifest().name.clone(), plugin);
        Ok(())
    }

    pub fn register_detector(
        &mut self,
        plugin: Arc<dyn Detector + Send + Sync>,
    ) -> anyhow::Result<()> {
        self.authorize(plugin.manifest())?;
        self.registry
            .detectors
            .insert(plugin.manifest().name.clone(), plugin);
        Ok(())
    }

    pub fn register_schema(&mut self, plugin: Arc<dyn Schema + Send + Sync>) -> anyhow::Result<()> {
        self.authorize(plugin.manifest())?;
        self.registry
            .schemas
            .insert(plugin.manifest().name.clone(), plugin);
        Ok(())
    }

    pub fn register_output(&mut self, plugin: Arc<dyn Output + Send + Sync>) -> anyhow::Result<()> {
        self.authorize(plugin.manifest())?;
        self.registry
            .outputs
            .insert(plugin.manifest().name.clone(), plugin);
        Ok(())
    }

    // --- resolution ------------------------------------------------------

    pub fn codec(&self, name: &str) -> Option<Arc<dyn Codec + Send + Sync>> {
        self.registry.codecs.get(name).cloned()
    }

    pub fn detector(&self, name: &str) -> Option<Arc<dyn Detector + Send + Sync>> {
        self.registry.detectors.get(name).cloned()
    }

    pub fn detectors(&self) -> Vec<Arc<dyn Detector + Send + Sync>> {
        self.registry.detectors.values().cloned().collect()
    }

    pub fn codec_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.registry.codecs.keys().cloned().collect();
        names.sort();
        names
    }

    fn has_builtin(&self, name: &str) -> bool {
        self.registry.codecs.contains_key(name)
            || self.registry.processors.contains_key(name)
            || self.registry.detectors.contains_key(name)
            || self.registry.schemas.contains_key(name)
            || self.registry.outputs.contains_key(name)
    }

    // --- declared-plugin lifecycle (DESIGN §11.4) ------------------------

    /// Validate config-declared plugins: capability grant + API compatibility +
    /// availability of the runtime for the declared `kind`.
    pub fn load_declared(&self, specs: &[PluginSpec]) -> Vec<PluginStatus> {
        specs
            .iter()
            .map(|spec| PluginStatus {
                name: spec.name.clone(),
                kind: spec.kind.clone(),
                state: self.evaluate(spec),
            })
            .collect()
    }

    fn evaluate(&self, spec: &PluginSpec) -> PluginState {
        // API compatibility.
        if let Some(api) = &spec.api {
            match ApiVersion::parse(api) {
                Some(v) if !self.api_version.supports(v) => {
                    return PluginState::Rejected(format!(
                        "targets API {v}, host implements {}",
                        self.api_version
                    ));
                }
                None => return PluginState::Rejected(format!("invalid API version '{api}'")),
                _ => {}
            }
        }
        // Capability grant.
        let requested = CapabilitySet::parse(&spec.capabilities);
        if let Some(missing) = self.granted.missing(requested.requested()) {
            return PluginState::Rejected(format!("ungranted capability '{}'", missing.token()));
        }
        // Runtime availability per kind.
        match spec.kind.as_str() {
            "builtin" | "compiled" | "" => {
                if self.has_builtin(&spec.name) {
                    PluginState::Registered
                } else {
                    PluginState::Rejected(format!("no compiled plugin named '{}'", spec.name))
                }
            }
            "wasm" => PluginState::Pending("wasm component host not yet available".into()),
            "grpc" | "sidecar" => {
                PluginState::Pending("gRPC sidecar host not yet available".into())
            }
            other => PluginState::Rejected(format!("unknown plugin kind '{other}'")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::KeywordDetector;
    use sigil_core::ecs;

    #[test]
    fn registers_and_resolves_builtin() {
        let mut host = PluginHost::with_safe_defaults();
        host.register_detector(Arc::new(KeywordDetector::new(
            "error-level",
            ecs::LOG_LEVEL,
            "error",
            50,
        )))
        .unwrap();
        assert!(host.detector("error-level").is_some());
        assert_eq!(host.detectors().len(), 1);
    }

    #[test]
    fn rejects_ungranted_capability() {
        let mut host = PluginHost::with_safe_defaults();
        // A detector manifest requesting network would be denied.
        let spec = PluginSpec {
            name: "needy".into(),
            kind: "wasm".into(),
            path: None,
            capabilities: vec!["network".into()],
            api: None,
        };
        let status = &host.load_declared(&[spec])[0];
        assert!(matches!(status.state, PluginState::Rejected(_)));
        let _ = &mut host;
    }

    #[test]
    fn declared_lifecycle_states() {
        let mut host = PluginHost::with_safe_defaults();
        host.register_detector(Arc::new(KeywordDetector::new(
            "error-level",
            ecs::LOG_LEVEL,
            "error",
            50,
        )))
        .unwrap();

        let specs = vec![
            PluginSpec {
                name: "error-level".into(),
                kind: "builtin".into(),
                path: None,
                capabilities: vec!["emit:signal".into()],
                api: Some("0.1".into()),
            },
            PluginSpec {
                name: "my_wasm".into(),
                kind: "wasm".into(),
                path: Some("./p.wasm".into()),
                capabilities: vec!["read:field:message".into()],
                api: None,
            },
            PluginSpec {
                name: "future".into(),
                kind: "builtin".into(),
                path: None,
                capabilities: vec![],
                api: Some("9.0".into()),
            },
        ];
        let statuses = host.load_declared(&specs);
        assert_eq!(statuses[0].state, PluginState::Registered);
        assert!(matches!(statuses[1].state, PluginState::Pending(_)));
        assert!(matches!(statuses[2].state, PluginState::Rejected(_))); // bad API
    }
}
