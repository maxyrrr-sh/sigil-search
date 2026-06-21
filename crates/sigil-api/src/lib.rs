//! `sigil-api` — HTTP API (DESIGN §10).
//!
//! Native search plus ecosystem compatibility (Phase 4): an
//! Elasticsearch-compatible `_bulk` ingest endpoint, a minimal ES `_search`
//! endpoint, and an OTLP/HTTP (JSON) logs receiver. Ecosystem ingest paths
//! normalize to ECS and index directly (they do not yet run the configurable
//! processing pipeline — a documented Phase 4 limitation).
#![allow(dead_code)]

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use sigil_core::{Codec, Event};
use sigil_index::{IndexedEvent, Indexer};
use sigil_ingest::JsonCodec;
use sigil_schema::{EcsSchema, Schema};

mod es;
mod otlp;

/// Build the full API router sharing one [`Indexer`].
pub fn router(indexer: Arc<Indexer>) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/search", get(search))
        .route("/sql", get(sql))
        .route("/query", get(query_dsl))
        .route("/_bulk", post(es::bulk))
        .route("/_search", get(es::search_get).post(es::search_post))
        .route("/v1/logs", post(otlp::logs))
        .with_state(indexer)
}

/// Bind `addr` and serve the API until the process is stopped.
pub async fn serve(indexer: Arc<Indexer>, addr: &str) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(indexer)).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Native search
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SearchParams {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn search(
    State(indexer): State<Arc<Indexer>>,
    Query(params): Query<SearchParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let q = params.q.unwrap_or_default();
    let limit = params.limit.unwrap_or(20);
    let hits = indexer.search(&q, limit)?;
    Ok(Json(json!({ "query": q, "count": hits.len(), "hits": hits })))
}

// ---------------------------------------------------------------------------
// SQL + pipe-DSL (DESIGN §9)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct QParam {
    #[serde(default)]
    q: Option<String>,
}

/// `GET /sql?q=SELECT ...` — DataFusion SQL over the `events` table.
async fn sql(
    State(indexer): State<Arc<Indexer>>,
    Query(params): Query<QParam>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let q = params.q.unwrap_or_default();
    let rows = sigil_query::QueryEngine::new(indexer).sql(&q).await?;
    Ok(Json(json!({ "count": rows.len(), "rows": rows })))
}

/// `GET /query?q=search ... | stats ...` — pipe-DSL lowered to SQL.
async fn query_dsl(
    State(indexer): State<Arc<Indexer>>,
    Query(params): Query<QParam>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let q = params.q.unwrap_or_default();
    let engine = sigil_query::QueryEngine::new(indexer);
    let sql = engine.explain_dsl(&q)?;
    let rows = engine.dsl(&q).await?;
    Ok(Json(json!({ "dsl": q, "sql": sql, "count": rows.len(), "rows": rows })))
}

// ---------------------------------------------------------------------------
// Shared helpers (used by the ES/OTLP modules)
// ---------------------------------------------------------------------------

/// Decode a JSON document and index it under `dataset` (ECS-normalized).
pub(crate) fn index_json_doc(indexer: &Indexer, dataset: &str, doc: &[u8]) -> anyhow::Result<()> {
    let codec = JsonCodec::default();
    let schema = EcsSchema::new(dataset);
    let records = codec
        .decode(doc)
        .map_err(|e| anyhow::anyhow!("decode: {e}"))?;
    for record in records {
        let event = schema
            .normalize(record)
            .map_err(|e| anyhow::anyhow!("normalize: {e}"))?;
        indexer.index(&event)?;
    }
    Ok(())
}

/// Normalize a pre-built record under `dataset` and index it.
pub(crate) fn index_event(indexer: &Indexer, event: &Event) -> anyhow::Result<()> {
    indexer.index(event)?;
    Ok(())
}

/// Rebuild a nested JSON object from an indexed event's dotted fields.
pub(crate) fn source_object(event: &IndexedEvent) -> serde_json::Value {
    let mut root = serde_json::Map::new();
    for (key, value) in &event.fields {
        let parts: Vec<&str> = key.split('.').collect();
        insert_nested(&mut root, &parts, value);
    }
    serde_json::Value::Object(root)
}

fn insert_nested(map: &mut serde_json::Map<String, serde_json::Value>, parts: &[&str], value: &str) {
    if parts.len() == 1 {
        map.insert(parts[0].to_string(), serde_json::Value::String(value.to_string()));
        return;
    }
    let entry = map
        .entry(parts[0].to_string())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if let serde_json::Value::Object(child) = entry {
        insert_nested(child, &parts[1..], value);
    } else {
        let mut child = serde_json::Map::new();
        insert_nested(&mut child, &parts[1..], value);
        *entry = serde_json::Value::Object(child);
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

pub(crate) struct ApiError(anyhow::Error);

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError(e)
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": self.0.to_string() })),
        )
            .into_response()
    }
}
