//! `sigil-search` — command-line entrypoint for the Sigil Search platform.
//!
//! Wires the configured inputs through codecs → ECS normalization → the
//! processing pipeline (enrich / mask / route, with dead-letter) → the indexer,
//! and serves the search/query API. See `docs/DESIGN.md`.

mod runtime;

use std::sync::Arc;
use std::time::Duration;

use sigil_cluster::{ClusterCatalog, Member, Role, RoleSet, ShardMap};
use sigil_config::Config;
use sigil_index::Indexer;

use runtime::{
    build_codec, build_host, build_pipeline, plugin_specs, BusSink, DeadLetter, DirectSink,
    EventSink, Ingestor, SignalSink,
};

const DEFAULT_CONFIG: &str = "./configs/sigil-search.yaml";

const USAGE: &str = "\
sigil-search - ELK-analog search & ingest platform

USAGE:
    sigil-search <command> [args]

COMMANDS:
    run [config]            Run the node (default config: ./configs/sigil-search.yaml)
    config validate [file]  Validate configuration against the schema
    config plan             Show the diff between desired and running state
    config apply            Apply configuration (hot-reload for safe changes)
    config diff             Show runtime drift vs declared config
    replay <file> [config]  Replay newline-delimited events through the pipeline
    tier roll [config]      Roll the hot tier to a cold Parquet segment
    tier prune [config]     Delete cold segments older than the cold retention
    cluster info [config]   Show this node's roles, shard map, and membership
    version                 Print version

See docs/DESIGN.md for the full design.";

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = dispatch(&args).await;
    std::process::exit(code);
}

async fn dispatch(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("version") => {
            println!("sigil-search {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some("run") => {
            let path = args.get(1).map(String::as_str).unwrap_or(DEFAULT_CONFIG);
            report(run(path).await)
        }
        Some("replay") => match args.get(1) {
            Some(file) => {
                let path = args.get(2).map(String::as_str).unwrap_or(DEFAULT_CONFIG);
                report(replay(file, path).await)
            }
            None => {
                eprintln!("replay needs a file argument\n\n{USAGE}");
                2
            }
        },
        Some("tier") => match args.get(1).map(String::as_str) {
            Some("roll") => {
                let path = args.get(2).map(String::as_str).unwrap_or(DEFAULT_CONFIG);
                report(tier_roll(path))
            }
            Some("prune") => {
                let path = args.get(2).map(String::as_str).unwrap_or(DEFAULT_CONFIG);
                report(tier_prune(path))
            }
            _ => {
                eprintln!("{USAGE}");
                2
            }
        },
        Some("cluster") => match args.get(1).map(String::as_str) {
            Some("info") => {
                let path = args.get(2).map(String::as_str).unwrap_or(DEFAULT_CONFIG);
                report(cluster_info(path))
            }
            _ => {
                eprintln!("{USAGE}");
                2
            }
        },
        Some("config") => match args.get(1).map(String::as_str) {
            Some("validate") => {
                let path = args.get(2).map(String::as_str).unwrap_or(DEFAULT_CONFIG);
                config_validate(path)
            }
            Some(sub @ ("plan" | "apply" | "diff")) => {
                eprintln!("[scaffold] `config {sub}` not implemented yet - see docs/DESIGN.md §12");
                1
            }
            _ => {
                eprintln!("{USAGE}");
                2
            }
        },
        Some("help") | Some("--help") | Some("-h") | None => {
            println!("{USAGE}");
            0
        }
        Some(other) => {
            eprintln!("unknown command: {other}\n\n{USAGE}");
            2
        }
    }
}

fn report(result: anyhow::Result<()>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    }
}

/// `config validate` — load and validate, printing any problems.
fn config_validate(path: &str) -> i32 {
    let cfg = match Config::load(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("invalid: {e:#}");
            return 1;
        }
    };
    let errors = cfg.validate();
    if errors.is_empty() {
        println!("{path}: valid");
        0
    } else {
        eprintln!("{path}: {} problem(s):", errors.len());
        for e in &errors {
            eprintln!("  - {e}");
        }
        1
    }
}

fn load_validated(path: &str) -> anyhow::Result<Config> {
    let cfg = Config::load(path)?;
    let errors = cfg.validate();
    if !errors.is_empty() {
        for e in &errors {
            eprintln!("config error: {e}");
        }
        anyhow::bail!("{} configuration problem(s); aborting", errors.len());
    }
    Ok(cfg)
}

/// `run` — start this node's configured roles (ingest / index / query).
async fn run(path: &str) -> anyhow::Result<()> {
    let cfg = load_validated(path)?;
    let roles = RoleSet::from_targets(&cfg.cluster.targets)?;
    let transport = sigil_cluster::build_transport(&cfg.cluster.transport_kind())?;
    let node_id = cfg
        .cluster
        .node_id
        .clone()
        .unwrap_or_else(|| format!("node-{}", std::process::id()));
    let shard_map = ShardMap::new(
        cfg.cluster.shards.unwrap_or(1),
        cfg.cluster.replicas.unwrap_or(1),
    );
    println!(
        "[sigil-search] node '{node_id}' roles={:?} transport={} shards={} replicas={}",
        roles.labels(),
        transport.kind(),
        shard_map.shards(),
        shard_map.replicas()
    );

    let indexer = Arc::new(Indexer::open(&cfg.index.dir)?);
    let dlq = Arc::new(DeadLetter::new(format!(
        "{}.deadletter.ndjson",
        cfg.index.dir.trim_end_matches('/')
    )));
    let topic = "events";

    // Plugin host (DESIGN §11): register first-party plugins, validate declared ones.
    let host = build_host()?;
    println!(
        "[sigil-search] plugin host: api {} | codecs {:?} | detectors {}",
        host.api_version(),
        host.codec_names(),
        host.detectors().len()
    );
    for status in host.load_declared(&plugin_specs(&cfg)) {
        println!("[sigil-search] plugin: {}", status.summary());
    }
    let detectors = host.detectors();
    let signals = Arc::new(SignalSink::new(format!(
        "{}.signals.ndjson",
        cfg.index.dir.trim_end_matches('/')
    )));

    // --- index role: consume the bus, commit, run tiering ------------------
    if roles.has(Role::Index) {
        println!("[sigil-search] index role: storing at {}", cfg.index.dir);

        // Subscribe before inputs start so no published events are missed.
        let mut rx = transport.subscribe(topic);
        let consumer = indexer.clone();
        tokio::spawn(async move {
            while let Some(bytes) = rx.recv().await {
                match sigil_cluster::wire::decode_event(&bytes) {
                    Ok(ev) => {
                        if let Err(e) = consumer.index(&ev) {
                            eprintln!("index error: {e}");
                        }
                    }
                    Err(e) => eprintln!("event decode error: {e}"),
                }
            }
        });

        // Commit ticker so buffered documents become searchable.
        let committer = indexer.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(500));
            loop {
                tick.tick().await;
                if let Err(e) = committer.commit() {
                    eprintln!("commit error: {e}");
                }
            }
        });

        // Optional hot→cold rollover + cold-tier pruning (Phase 2).
        if let Some(secs) = cfg.index.rollover_secs.filter(|s| *s > 0) {
            let indexer = indexer.clone();
            let cold_secs = cfg.cold_retention_secs();
            println!("[sigil-search] auto-rollover every {secs}s");
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_secs(secs));
                tick.tick().await; // skip the immediate first tick
                loop {
                    tick.tick().await;
                    match indexer.archive() {
                        Ok(Some(seg)) => println!(
                            "[sigil-search] rolled {} events to cold segment #{}",
                            seg.count, seg.id
                        ),
                        Ok(None) => {}
                        Err(e) => eprintln!("rollover error: {e}"),
                    }
                    if let Some(cs) = cold_secs {
                        if let Ok(n @ 1..) = indexer.prune_cold(cs) {
                            println!("[sigil-search] pruned {n} expired cold segment(s)");
                        }
                    }
                }
            });
        }
    }

    // --- ingest role: run inputs that publish onto the bus -----------------
    if roles.has(Role::Ingest) {
        let sink: Arc<dyn EventSink> = Arc::new(BusSink {
            transport: transport.clone(),
            topic: topic.to_string(),
        });
        let mut started = 0;
        for input in cfg.inputs.clone() {
            match input.kind.as_str() {
                "syslog" | "file" => {
                    let ingestor = Arc::new(Ingestor::new(
                        &input.id,
                        build_codec(&host, &input)?,
                        build_pipeline(&cfg, &input.id)?,
                        sink.clone(),
                        dlq.clone(),
                        detectors.clone(),
                        signals.clone(),
                    ));
                    if input.kind == "syslog" {
                        runtime::spawn_syslog(input, ingestor);
                    } else {
                        runtime::spawn_file(input, ingestor);
                    }
                    started += 1;
                }
                // Ecosystem HTTP inputs serve the API router (ES `_bulk`/`_search`,
                // OTLP `/v1/logs`) on their own listen port, writing directly to
                // the local index.
                "http_bulk" | "otlp" => match input.listen.clone() {
                    Some(addr) => {
                        let indexer = indexer.clone();
                        let (id, kind) = (input.id.clone(), input.kind.clone());
                        println!("[sigil-search] {kind} input '{id}' serving HTTP on http://{addr}  (/_bulk, /_search, /v1/logs)");
                        tokio::spawn(async move {
                            if let Err(e) = sigil_api::serve(indexer, &addr).await {
                                eprintln!("{kind} input '{id}': {e}");
                            }
                        });
                        started += 1;
                    }
                    None => eprintln!(
                        "[sigil-search] {} input '{}' needs a listen address; skipping",
                        input.kind, input.id
                    ),
                },
                other => {
                    eprintln!("[sigil-search] input '{}' type '{other}' not supported; skipping", input.id);
                }
            }
        }
        println!("[sigil-search] ingest role: started {started} input(s)");
    }

    // --- query role: serve the search / SQL / DSL API ----------------------
    if roles.has(Role::Query) {
        let indexer = indexer.clone();
        let addr = cfg.api.listen.clone();
        println!("[sigil-search] query role: API on http://{addr}  (/search, /sql, /query)");
        tokio::spawn(async move {
            if let Err(e) = sigil_api::serve(indexer, &addr).await {
                eprintln!("api error: {e}");
            }
        });
    }

    println!("[sigil-search] running; press Ctrl-C to stop");
    tokio::signal::ctrl_c().await?;
    println!(
        "\n[sigil-search] shutting down ({} dead-lettered, {} signal(s))",
        dlq.count(),
        signals.count()
    );
    indexer.commit().ok();
    Ok(())
}

/// `cluster info` — print this node's roles, shard map, and (single-node) membership.
fn cluster_info(path: &str) -> anyhow::Result<()> {
    let cfg = load_validated(path)?;
    let roles = RoleSet::from_targets(&cfg.cluster.targets)?;
    let node_id = cfg
        .cluster
        .node_id
        .clone()
        .unwrap_or_else(|| format!("node-{}", std::process::id()));
    let shard_map = ShardMap::new(
        cfg.cluster.shards.unwrap_or(1),
        cfg.cluster.replicas.unwrap_or(1),
    );
    let member = Member {
        node_id,
        roles: roles.labels().iter().map(|s| s.to_string()).collect(),
        addr: Some(cfg.api.listen.clone()),
    };
    let catalog = ClusterCatalog::new(member, shard_map);
    println!("{}", serde_json::to_string_pretty(&catalog.snapshot())?);
    println!("transport: {}", cfg.cluster.transport_kind());
    Ok(())
}

/// `tier roll` — force a hot→cold rollover.
fn tier_roll(path: &str) -> anyhow::Result<()> {
    let cfg = load_validated(path)?;
    let indexer = Indexer::open(&cfg.index.dir)?;
    match indexer.archive()? {
        Some(seg) => println!(
            "rolled {} events to cold segment #{} ({})",
            seg.count, seg.id, seg.path
        ),
        None => println!("nothing to roll (hot tier is empty)"),
    }
    Ok(())
}

/// `tier prune` — drop cold segments older than the configured cold retention.
fn tier_prune(path: &str) -> anyhow::Result<()> {
    let cfg = load_validated(path)?;
    let indexer = Indexer::open(&cfg.index.dir)?;
    let secs = cfg.cold_retention_secs().unwrap_or(365 * 86_400);
    let removed = indexer.prune_cold(secs)?;
    println!("pruned {removed} cold segment(s) older than {secs}s");
    Ok(())
}

/// `replay` — push each line of a file through the default pipeline and index it.
async fn replay(file: &str, config: &str) -> anyhow::Result<()> {
    let cfg = load_validated(config)?;
    let indexer = Arc::new(Indexer::open(&cfg.index.dir)?);
    let dlq = Arc::new(DeadLetter::new(format!(
        "{}.deadletter.ndjson",
        cfg.index.dir.trim_end_matches('/')
    )));

    let input = sigil_config::Input {
        id: "replay".to_string(),
        kind: "file".to_string(),
        listen: None,
        path: Some(file.to_string()),
        codec: None,
    };
    let host = build_host()?;
    let signals = Arc::new(SignalSink::new(format!(
        "{}.signals.ndjson",
        cfg.index.dir.trim_end_matches('/')
    )));
    let sink: Arc<dyn EventSink> = Arc::new(DirectSink(indexer.clone()));
    let ingestor = Ingestor::new(
        "replay",
        build_codec(&host, &input)?,
        build_pipeline(&cfg, "replay")?,
        sink,
        dlq.clone(),
        host.detectors(),
        signals.clone(),
    );

    let bytes = tokio::fs::read(file).await?;
    let count = ingestor.ingest_lines(&bytes);
    indexer.commit()?;
    println!(
        "replayed {count} line(s) from {file} ({} dead-lettered, {} signal(s))",
        dlq.count(),
        signals.count()
    );
    Ok(())
}
