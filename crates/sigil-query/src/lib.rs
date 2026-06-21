//! `sigil-query` — unified query engine (DESIGN §9, ADR 2).
//!
//! Three front-ends over one dataset:
//! * **query-string** — Lucene-style full-text, served by the Tantivy index.
//! * **SQL** — analytical queries via DataFusion over an `events` table built
//!   from the hot index (and registered cold Parquet segments).
//! * **pipe-DSL** — an SPL/KQL-style `a | b | c` syntax lowered to SQL.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;

use datafusion::arrow::array::{
    ArrayRef, BooleanArray, Float64Array, Int64Array, StringArray, UInt64Array,
};
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::prelude::*;
use serde_json::{json, Value};
use sigil_index::{Hit, IndexedEvent, Indexer};

pub mod dsl;

const DEFAULT_MAX_ROWS: usize = 200_000;

/// The unified query engine. Cheap to construct (wraps an [`Indexer`] handle).
pub struct QueryEngine {
    indexer: Arc<Indexer>,
    cold: Vec<PathBuf>,
    max_rows: usize,
}

impl QueryEngine {
    pub fn new(indexer: Arc<Indexer>) -> Self {
        QueryEngine {
            indexer,
            cold: Vec::new(),
            max_rows: DEFAULT_MAX_ROWS,
        }
    }

    /// Register cold Parquet segments (Phase 2) to be unioned into `events`.
    pub fn with_cold(mut self, segments: Vec<PathBuf>) -> Self {
        self.cold = segments;
        self
    }

    /// Lucene-style full-text search (delegates to the Tantivy index).
    pub fn query_string(&self, query: &str, limit: usize) -> anyhow::Result<Vec<Hit>> {
        self.indexer.search(query, limit)
    }

    /// Run a SQL query against the `events` table; rows come back as JSON.
    pub async fn sql(&self, query: &str) -> anyhow::Result<Vec<Value>> {
        let ctx = self.context().await?;
        let df = ctx.sql(query).await?;
        let batches = df.collect().await?;
        Ok(batches_to_json(&batches))
    }

    /// Run a pipe-DSL query (lowered to SQL).
    pub async fn dsl(&self, pipeline: &str) -> anyhow::Result<Vec<Value>> {
        let sql = dsl::lower(pipeline)?;
        self.sql(&sql).await
    }

    /// The SQL the DSL lowers to (exposed for explain/debugging).
    pub fn explain_dsl(&self, pipeline: &str) -> anyhow::Result<String> {
        dsl::lower(pipeline)
    }

    async fn context(&self) -> anyhow::Result<SessionContext> {
        let ctx = SessionContext::new();
        let batch = events_batch(&self.indexer.dump(self.max_rows)?)?;
        // Use explicitly-set cold segments, else discover them from the catalog.
        let cold = if self.cold.is_empty() {
            self.indexer.cold_paths()
        } else {
            self.cold.clone()
        };
        if cold.is_empty() {
            ctx.register_batch("events", batch)?;
        } else {
            ctx.register_batch("hot", batch)?;
            let mut parts = vec!["SELECT * FROM hot".to_string()];
            for (i, path) in cold.iter().enumerate() {
                let name = format!("cold_{i}");
                let p = path.to_string_lossy().to_string();
                ctx.register_parquet(&name, &p, ParquetReadOptions::default())
                    .await?;
                parts.push(format!("SELECT * FROM {name}"));
            }
            ctx.sql(&format!("CREATE VIEW events AS {}", parts.join(" UNION ALL ")))
                .await?;
        }
        Ok(ctx)
    }
}

/// The columnar schema of the `events` table. Cold Parquet segments are written
/// with this exact schema so they union cleanly with the hot rows.
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

fn events_batch(events: &[IndexedEvent]) -> anyhow::Result<RecordBatch> {
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

fn batches_to_json(batches: &[RecordBatch]) -> Vec<Value> {
    let mut rows = Vec::new();
    for batch in batches {
        let schema = batch.schema();
        for row in 0..batch.num_rows() {
            let mut obj = serde_json::Map::new();
            for (i, f) in schema.fields().iter().enumerate() {
                obj.insert(f.name().clone(), cell(batch.column(i), row));
            }
            rows.push(Value::Object(obj));
        }
    }
    rows
}

fn cell(col: &ArrayRef, row: usize) -> Value {
    if col.is_null(row) {
        return Value::Null;
    }
    let any = col.as_any();
    if let Some(a) = any.downcast_ref::<Int64Array>() {
        return json!(a.value(row));
    }
    if let Some(a) = any.downcast_ref::<UInt64Array>() {
        return json!(a.value(row));
    }
    if let Some(a) = any.downcast_ref::<Float64Array>() {
        return json!(a.value(row));
    }
    if let Some(a) = any.downcast_ref::<BooleanArray>() {
        return json!(a.value(row));
    }
    if let Some(a) = any.downcast_ref::<StringArray>() {
        return json!(a.value(row));
    }
    // Fallback: stringify any other arrow type.
    use datafusion::arrow::util::display::{ArrayFormatter, FormatOptions};
    match ArrayFormatter::try_new(col, &FormatOptions::default()) {
        Ok(f) => Value::String(f.value(row).to_string()),
        Err(_) => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sigil_core::{ecs, Event};

    fn indexer_with(events: Vec<(&str, &str, &str)>) -> Arc<Indexer> {
        let idx = Indexer::in_memory().unwrap();
        for (dataset, message, level) in events {
            let mut ev = Event {
                dataset: dataset.to_string(),
                ..Default::default()
            };
            ev.set(ecs::MESSAGE, message);
            ev.set(ecs::LOG_LEVEL, level);
            idx.index(&ev).unwrap();
        }
        idx.commit().unwrap();
        Arc::new(idx)
    }

    #[tokio::test]
    async fn sql_group_by() {
        let engine = QueryEngine::new(indexer_with(vec![
            ("app", "a", "error"),
            ("app", "b", "error"),
            ("app", "c", "info"),
        ]));
        let rows = engine
            .sql("SELECT log_level, count(*) as c FROM events GROUP BY log_level ORDER BY c DESC")
            .await
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["log_level"], "error");
        assert_eq!(rows[0]["c"], 2);
    }

    #[tokio::test]
    async fn dsl_lowers_and_runs() {
        let engine = QueryEngine::new(indexer_with(vec![
            ("app", "login ok", "info"),
            ("app", "login failed", "error"),
        ]));
        let rows = engine
            .dsl("search login | where log.level == error | head 10")
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["message"], "login failed");
    }
}
