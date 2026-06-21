//! Runtime wiring: build codecs and the processing pipeline from config, run
//! inputs, and route processed events to the index or the dead-letter sink.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use sigil_cluster::{wire, Transport};
use sigil_config::{Config, Input};
use sigil_core::{ecs, Codec, Detector, Event, Signal};
use sigil_index::Indexer;
use sigil_ingest::{
    CefCodec, Condition, CsvCodec, Dest, GeoIp, GrokCodec, JsonCodec, KvCodec, PiiMask, Pipeline,
    RegexCodec, SyslogCodec, TemplateMiner,
};
use sigil_plugin::{Capability, CapabilitySet, KeywordDetector, PluginHost};
use sigil_schema::{EcsSchema, Schema};

type ArcCodec = Arc<dyn Codec + Send + Sync>;
type Detectors = Vec<Arc<dyn Detector + Send + Sync>>;

fn to_anyhow(e: sigil_core::Error) -> anyhow::Error {
    anyhow::anyhow!("{e}")
}

// ---------------------------------------------------------------------------
// Plugin host: register first-party plugins (DESIGN §11)
// ---------------------------------------------------------------------------

/// Build the plugin host with first-party plugins registered. First-party
/// plugins are trusted with field access + operational capabilities, but not
/// network.
pub fn build_host() -> anyhow::Result<PluginHost> {
    let granted = CapabilitySet::new(vec![
        Capability::ReadField("*".into()),
        Capability::WriteField("*".into()),
        Capability::EmitSignal,
        Capability::Other("decode".into()),
        Capability::Other("normalize".into()),
        Capability::Other("process".into()),
    ]);
    let mut host = PluginHost::new(granted);
    host.register_codec(Arc::new(JsonCodec::default()))?;
    host.register_codec(Arc::new(SyslogCodec::default()))?;
    host.register_codec(Arc::new(KvCodec::default()))?;
    host.register_codec(Arc::new(CefCodec::default()))?;
    // Example detector: a generic substring match flagging error-level events.
    host.register_detector(Arc::new(KeywordDetector::new(
        "error-level",
        ecs::LOG_LEVEL,
        "error",
        50,
    )))?;
    Ok(host)
}

/// Map config-declared plugins to the host's spec type.
pub fn plugin_specs(cfg: &Config) -> Vec<sigil_plugin::PluginSpec> {
    cfg.plugins
        .iter()
        .map(|p| sigil_plugin::PluginSpec {
            name: p.name.clone(),
            kind: p.kind.clone().unwrap_or_else(|| "builtin".to_string()),
            path: p.path.clone(),
            capabilities: p.capabilities.clone(),
            api: None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Event sinks: where Index-destined events go after the pipeline
// ---------------------------------------------------------------------------

/// Destination for Index-routed events. In monolith/scale-out `run`, events flow
/// ingest → transport → index; `replay` writes the index directly.
pub trait EventSink: Send + Sync {
    fn deliver(&self, event: &Event);
}

/// Publish events onto the transport bus (consumed by the `index` role).
pub struct BusSink {
    pub transport: Arc<dyn Transport>,
    pub topic: String,
}

impl EventSink for BusSink {
    fn deliver(&self, event: &Event) {
        if let Err(e) = self.transport.publish(&self.topic, &wire::encode_event(event)) {
            eprintln!("transport publish error: {e}");
        }
    }
}

/// Write events straight to the index (used by `replay`).
pub struct DirectSink(pub Arc<Indexer>);

impl EventSink for DirectSink {
    fn deliver(&self, event: &Event) {
        if let Err(e) = self.0.index(event) {
            eprintln!("index error: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Dead-letter sink
// ---------------------------------------------------------------------------

/// Append-only NDJSON sink for events that fail to decode/normalize or are
/// routed to dead-letter by the pipeline.
pub struct DeadLetter {
    path: String,
    count: AtomicU64,
    file: Mutex<Option<std::fs::File>>,
}

impl DeadLetter {
    pub fn new(path: String) -> Self {
        DeadLetter {
            path,
            count: AtomicU64::new(0),
            file: Mutex::new(None),
        }
    }

    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    fn write_line(&self, line: &str) {
        use std::io::Write;
        let mut guard = self.file.lock().expect("dlq poisoned");
        if guard.is_none() {
            *guard = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .ok();
        }
        if let Some(f) = guard.as_mut() {
            let _ = writeln!(f, "{line}");
        }
    }

    pub fn record_raw(&self, reason: &str, raw: &[u8]) {
        self.count.fetch_add(1, Ordering::Relaxed);
        let mut obj = serde_json::Map::new();
        obj.insert("reason".into(), reason.into());
        obj.insert(
            "raw".into(),
            String::from_utf8_lossy(raw).into_owned().into(),
        );
        self.write_line(&serde_json::Value::Object(obj).to_string());
    }

    pub fn record_event(&self, reason: &str, ev: &Event) {
        self.count.fetch_add(1, Ordering::Relaxed);
        let mut fields = serde_json::Map::new();
        for (k, v) in &ev.fields {
            fields.insert(k.clone(), v.clone().into());
        }
        let mut obj = serde_json::Map::new();
        obj.insert("reason".into(), reason.into());
        obj.insert("dataset".into(), ev.dataset.clone().into());
        obj.insert("fields".into(), serde_json::Value::Object(fields));
        self.write_line(&serde_json::Value::Object(obj).to_string());
    }
}

// ---------------------------------------------------------------------------
// Signal sink: detection signals emitted by Detector plugins
// ---------------------------------------------------------------------------

/// Append-only NDJSON sink for [`Signal`]s emitted by detector plugins.
pub struct SignalSink {
    path: String,
    count: AtomicU64,
    file: Mutex<Option<std::fs::File>>,
}

impl SignalSink {
    pub fn new(path: String) -> Self {
        SignalSink {
            path,
            count: AtomicU64::new(0),
            file: Mutex::new(None),
        }
    }

    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    pub fn record(&self, signal: &Signal) {
        use std::io::Write;
        self.count.fetch_add(1, Ordering::Relaxed);
        let mut fields = serde_json::Map::new();
        for (k, v) in &signal.fields {
            fields.insert(k.clone(), v.clone().into());
        }
        let mut obj = serde_json::Map::new();
        obj.insert("source".into(), signal.source.clone().into());
        obj.insert("severity".into(), signal.severity.into());
        obj.insert("events".into(), signal.events.clone().into());
        obj.insert("fields".into(), serde_json::Value::Object(fields));
        let line = serde_json::Value::Object(obj).to_string();

        let mut guard = self.file.lock().expect("signals poisoned");
        if guard.is_none() {
            *guard = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .ok();
        }
        if let Some(f) = guard.as_mut() {
            let _ = writeln!(f, "{line}");
        }
    }
}

// ---------------------------------------------------------------------------
// Ingestor: codec → schema → pipeline → detect → index/dead-letter
// ---------------------------------------------------------------------------

pub struct Ingestor {
    codec: ArcCodec,
    schema: EcsSchema,
    pipeline: Pipeline,
    sink: Arc<dyn EventSink>,
    dlq: Arc<DeadLetter>,
    detectors: Detectors,
    signals: Arc<SignalSink>,
}

impl Ingestor {
    pub fn new(
        dataset: &str,
        codec: ArcCodec,
        pipeline: Pipeline,
        sink: Arc<dyn EventSink>,
        dlq: Arc<DeadLetter>,
        detectors: Detectors,
        signals: Arc<SignalSink>,
    ) -> Self {
        Ingestor {
            codec,
            schema: EcsSchema::new(dataset),
            pipeline,
            sink,
            dlq,
            detectors,
            signals,
        }
    }

    /// Decode → normalize → run pipeline → route. Failures go to dead-letter.
    pub fn ingest_bytes(&self, bytes: &[u8]) {
        let records = match self.codec.decode(bytes) {
            Ok(r) => r,
            Err(e) => {
                self.dlq.record_raw(&format!("decode: {e}"), bytes);
                return;
            }
        };
        for record in records {
            let event = match self.schema.normalize(record) {
                Ok(ev) => ev,
                Err(e) => {
                    self.dlq.record_raw(&format!("normalize: {e}"), bytes);
                    continue;
                }
            };
            for (ev, dest) in self.pipeline.run(event).routed {
                match dest {
                    Dest::Index => {
                        // Run detector plugins, emitting signals (the SIEM hook).
                        for detector in &self.detectors {
                            if let Some(signal) = detector.eval(&ev) {
                                self.signals.record(&signal);
                            }
                        }
                        self.sink.deliver(&ev);
                    }
                    Dest::DeadLetter => self.dlq.record_event("routed", &ev),
                    Dest::Drop => {}
                }
            }
        }
    }

    /// Ingest newline-delimited input, returning the number of non-empty lines.
    pub fn ingest_lines(&self, bytes: &[u8]) -> usize {
        let mut count = 0;
        for line in bytes.split(|b| *b == b'\n') {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.is_empty() {
                continue;
            }
            self.ingest_bytes(line);
            count += 1;
        }
        count
    }
}

// ---------------------------------------------------------------------------
// Builders
// ---------------------------------------------------------------------------

/// Build a codec for an input, resolving config-free codecs from the plugin
/// host's registry and constructing parameterized codecs (csv/regex/grok)
/// directly.
pub fn build_codec(host: &PluginHost, input: &Input) -> anyhow::Result<ArcCodec> {
    let kind = input
        .codec
        .as_ref()
        .map(|c| c.kind.as_str())
        .unwrap_or(match input.kind.as_str() {
            "syslog" => "syslog",
            _ => "json",
        });

    if let Some(codec) = host.codec(kind) {
        return Ok(codec);
    }
    match kind {
        "csv" => {
            let columns = input
                .codec
                .as_ref()
                .and_then(|c| c.columns.clone())
                .unwrap_or_default();
            Ok(Arc::new(CsvCodec::new(columns)))
        }
        "regex" => {
            let pattern = codec_pattern(input, "regex")?;
            Ok(Arc::new(RegexCodec::new(&pattern).map_err(to_anyhow)?))
        }
        "grok" => {
            let pattern = codec_pattern(input, "grok")?;
            Ok(Arc::new(GrokCodec::new(&pattern).map_err(to_anyhow)?))
        }
        other => anyhow::bail!("input '{}' has unknown codec '{other}'", input.id),
    }
}

fn codec_pattern(input: &Input, kind: &str) -> anyhow::Result<String> {
    input
        .codec
        .as_ref()
        .and_then(|c| c.pattern.clone())
        .ok_or_else(|| anyhow::anyhow!("codec '{kind}' on input '{}' needs a `pattern`", input.id))
}

/// Build the processing pipeline for `input_id` from the config (the first
/// pipeline whose `from` lists the input; otherwise an index-everything default).
pub fn build_pipeline(cfg: &Config, input_id: &str) -> anyhow::Result<Pipeline> {
    let mut pipe = Pipeline::new();
    let Some(spec) = cfg
        .pipelines
        .iter()
        .find(|p| p.from.iter().any(|f| f == input_id))
    else {
        return Ok(pipe);
    };

    for step in &spec.steps {
        pipe = apply_step(pipe, step)?;
    }
    for route in &spec.route {
        pipe = apply_route(pipe, route)?;
    }
    Ok(pipe)
}

fn condition(value: Option<&serde_yaml::Value>) -> anyhow::Result<Condition> {
    match value.and_then(|v| v.as_str()) {
        Some(s) => Condition::parse(s).map_err(to_anyhow),
        None => Ok(Condition::Always),
    }
}

fn apply_step(mut pipe: Pipeline, step: &serde_yaml::Value) -> anyhow::Result<Pipeline> {
    // A bare string step, e.g. `- drain`.
    if let Some(s) = step.as_str() {
        return Ok(match s {
            "drain" => pipe.with_processor(Condition::Always, Box::new(TemplateMiner::default())),
            other => {
                eprintln!("[sigil-search] unknown pipeline step '{other}'; ignoring");
                pipe
            }
        });
    }

    let map = step
        .as_mapping()
        .ok_or_else(|| anyhow::anyhow!("pipeline step must be a string or mapping"))?;
    for (key, value) in map {
        let key = key.as_str().unwrap_or_default();
        pipe = match key {
            // Normalization is performed by the Ingestor's schema (ECS-only today).
            "normalize" => {
                if let Some(schema) = value.get("schema").and_then(|v| v.as_str()) {
                    if schema != "ecs" {
                        eprintln!("[sigil-search] schema '{schema}' not available; using ecs");
                    }
                }
                pipe
            }
            "drain" => pipe.with_processor(Condition::Always, Box::new(TemplateMiner::default())),
            "enrich" => apply_enrich(pipe, value),
            "mask" => apply_mask(pipe, value)?,
            "set" => {
                let field = value
                    .get("field")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("`set` step needs a `field`"))?;
                let val = value.get("value").and_then(|v| v.as_str()).unwrap_or("");
                let when = condition(value.get("when"))?;
                pipe.with_set(when, field, val)
            }
            "drop" => {
                let when = condition(value.get("when"))?;
                pipe.with_drop(when)
            }
            other => {
                eprintln!("[sigil-search] unknown pipeline step '{other}'; ignoring");
                pipe
            }
        };
    }
    Ok(pipe)
}

fn apply_enrich(mut pipe: Pipeline, value: &serde_yaml::Value) -> Pipeline {
    let names = value.as_sequence().cloned().unwrap_or_default();
    for name in names {
        match name.as_str() {
            Some("geoip") => {
                pipe = pipe.with_processor(Condition::Always, Box::new(GeoIp::default_fields()));
            }
            Some(other) => {
                eprintln!("[sigil-search] enrichment '{other}' not available in Phase 1; skipping");
            }
            None => {}
        }
    }
    pipe
}

fn apply_mask(pipe: Pipeline, value: &serde_yaml::Value) -> anyhow::Result<Pipeline> {
    let fields = value
        .get("fields")
        .and_then(|v| v.as_sequence())
        .map(|s| {
            s.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut mask = PiiMask::builtin(fields);
    if let Some(patterns) = value.get("patterns").and_then(|v| v.as_sequence()) {
        for p in patterns.iter().filter_map(|v| v.as_str()) {
            mask = mask.with_pattern(p).map_err(to_anyhow)?;
        }
    }
    let when = condition(value.get("when"))?;
    Ok(pipe.with_processor(when, Box::new(mask)))
}

fn apply_route(pipe: Pipeline, route: &serde_yaml::Value) -> anyhow::Result<Pipeline> {
    let to = route.get("to").and_then(|v| v.as_str()).unwrap_or("index");
    let when = condition(route.get("when"))?;
    let dest = match to {
        "index" => Dest::Index,
        "drop" => Dest::Drop,
        "deadletter" | "dead_letter" => Dest::DeadLetter,
        other => {
            eprintln!("[sigil-search] unknown route target '{other}'; routing to index");
            Dest::Index
        }
    };
    Ok(pipe.with_route(when, dest))
}

// ---------------------------------------------------------------------------
// Input runners
// ---------------------------------------------------------------------------

/// Spawn a UDP syslog listener feeding `ingestor`.
pub fn spawn_syslog(input: Input, ingestor: Arc<Ingestor>) {
    tokio::spawn(async move {
        let addr = match input.listen.clone() {
            Some(a) => a,
            None => {
                eprintln!("syslog input '{}' has no listen address", input.id);
                return;
            }
        };
        let socket = match tokio::net::UdpSocket::bind(&addr).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("syslog input '{}' bind {addr}: {e}", input.id);
                return;
            }
        };
        println!("[sigil-search] syslog '{}' listening on udp://{addr}", input.id);
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match socket.recv_from(&mut buf).await {
                Ok((n, _)) => {
                    ingestor.ingest_lines(&buf[..n]);
                }
                Err(e) => {
                    eprintln!("syslog input '{}' recv: {e}", input.id);
                    return;
                }
            }
        }
    });
}

/// Spawn a one-shot file reader feeding `ingestor`.
pub fn spawn_file(input: Input, ingestor: Arc<Ingestor>) {
    tokio::spawn(async move {
        let path = match input.path.clone() {
            Some(p) => p,
            None => {
                eprintln!("file input '{}' has no path", input.id);
                return;
            }
        };
        match tokio::fs::read(&path).await {
            Ok(bytes) => {
                let n = ingestor.ingest_lines(&bytes);
                println!("[sigil-search] file '{}' ingested {n} line(s) from {path}", input.id);
            }
            Err(e) => eprintln!("file input '{}' read {path}: {e}", input.id),
        }
    });
}
