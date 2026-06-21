//! Tiered storage (DESIGN §8): cold Parquet segments + a catalog.
//!
//! A *rollover* writes the current hot events to a Parquet **segment** and
//! records it in the [`Catalog`] (segment id, time range, row count). The query
//! engine registers these segments alongside the hot rows. The Parquet schema
//! here must stay identical to `sigil_query::events_schema` so the two union.

use std::path::Path;
use std::sync::Arc;

use datafusion::arrow::array::{ArrayRef, Int64Array, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::parquet::arrow::ArrowWriter;
use datafusion::parquet::file::properties::WriterProperties;
use serde::{Deserialize, Serialize};

use crate::IndexedEvent;

/// Metadata for one cold Parquet segment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub id: u64,
    pub path: String,
    pub min_ts: i64,
    pub max_ts: i64,
    pub count: usize,
    pub created_ts: i64,
}

/// The segment catalog (persisted as `catalog.json` in the index dir).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Catalog {
    #[serde(default)]
    pub segments: Vec<Segment>,
    #[serde(default)]
    pub next_id: u64,
}

impl Catalog {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        std::fs::write(path, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
}

/// The columnar schema for cold segments (mirrors `sigil_query::events_schema`).
pub fn events_schema() -> Schema {
    Schema::new(vec![
        Field::new("ts", DataType::Int64, true),
        Field::new("ingest_ts", DataType::Int64, true),
        Field::new("id", DataType::Utf8, true),
        Field::new("dataset", DataType::Utf8, true),
        Field::new("message", DataType::Utf8, true),
        Field::new("host", DataType::Utf8, true),
        Field::new("log_level", DataType::Utf8, true),
    ])
}

/// Write events to a Parquet segment file.
pub fn write_parquet(path: &Path, events: &[IndexedEvent]) -> anyhow::Result<()> {
    let batch = build_batch(events)?;
    let file = std::fs::File::create(path)?;
    let props = WriterProperties::builder().build();
    let mut writer = ArrowWriter::try_new(file, Arc::new(events_schema()), Some(props))?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

fn build_batch(events: &[IndexedEvent]) -> anyhow::Result<RecordBatch> {
    let field = |e: &IndexedEvent, key: &str| {
        e.fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };
    let ts: Int64Array = events.iter().map(|e| Some(e.ts)).collect();
    let ingest_ts: Int64Array = events.iter().map(|e| Some(e.ingest_ts)).collect();
    let id: StringArray = events.iter().map(|e| Some(e.id.clone())).collect();
    let dataset: StringArray = events.iter().map(|e| Some(e.dataset.clone())).collect();
    let message: StringArray = events.iter().map(|e| field(e, "message")).collect();
    let host: StringArray = events.iter().map(|e| field(e, "host.name")).collect();
    let log_level: StringArray = events.iter().map(|e| field(e, "log.level")).collect();

    let columns: Vec<ArrayRef> = vec![
        Arc::new(ts),
        Arc::new(ingest_ts),
        Arc::new(id),
        Arc::new(dataset),
        Arc::new(message),
        Arc::new(host),
        Arc::new(log_level),
    ];
    Ok(RecordBatch::try_new(Arc::new(events_schema()), columns)?)
}
