//! `sigil-plugin` — the plugin host: the stable extension surface (DESIGN §11).
//!
//! This is how the platform grows (and how `sigil-siem` is built) without
//! forking the core. A [`PluginHost`] holds a typed registry of compile-time
//! plugins (the `sigil-core` traits: [`Codec`](sigil_core::Codec),
//! [`Processor`](sigil_core::Processor), [`Detector`](sigil_core::Detector),
//! [`Schema`](sigil_core::Schema), [`Output`](sigil_core::Output)), enforces
//! [`capability`]-based permissions and [`version`] compatibility, validates
//! [`manifest`]s, and runs the lifecycle for configuration-declared plugins.
//! [`contracts`] holds the conformance checks that back the compatibility
//! guarantee.
//!
//! **Provided + tested:** the registry, capabilities, versioning, manifest
//! digest, contract tests, and an example [`KeywordDetector`].
//! **Deferred (need a build toolchain / sidecar to test):** the wasmtime
//! Component Model host and the gRPC + Arrow Flight sidecar runtime — declared
//! `wasm`/`grpc` plugins validate and report as *pending*.
#![allow(dead_code)]

pub mod capability;
pub mod contracts;
pub mod detector;
pub mod manifest;
pub mod registry;
pub mod version;

pub use capability::{Capability, CapabilitySet};
pub use detector::KeywordDetector;
pub use registry::{PluginHost, PluginSpec, PluginState, PluginStatus};
pub use version::ApiVersion;
