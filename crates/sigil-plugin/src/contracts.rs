//! Contract tests for plugin extension points (DESIGN §11.4).
//!
//! These are the conformance checks the ecosystem runs against a plugin to
//! confirm it honors an extension-point contract — the basis of the
//! backward-compatibility guarantee. Helpers return `Err` on a violation.

use sigil_core::{Codec, Detector, Event, Signal};

/// A [`Codec`] must decode a sample without error and carry a named manifest.
pub fn codec_contract(codec: &dyn Codec, sample: &[u8]) -> anyhow::Result<usize> {
    if codec.manifest().name.trim().is_empty() {
        anyhow::bail!("codec manifest is missing a name");
    }
    let records = codec
        .decode(sample)
        .map_err(|e| anyhow::anyhow!("codec '{}' failed to decode sample: {e}", codec.manifest().name))?;
    Ok(records.len())
}

/// A [`Detector`] must not panic and, when it emits, the [`Signal`] must name a
/// source.
pub fn detector_contract(detector: &dyn Detector, event: &Event) -> anyhow::Result<Option<Signal>> {
    let signal = detector.eval(event);
    if let Some(s) = &signal {
        if s.source.trim().is_empty() {
            anyhow::bail!(
                "detector '{}' emitted a signal with an empty source",
                detector.manifest().name
            );
        }
    }
    Ok(signal)
}
