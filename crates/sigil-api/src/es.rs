//! Elasticsearch-compatible endpoints (DESIGN §10, ADR 5): `_bulk` ingest and a
//! minimal `_search`. Enough for Beats/Logstash/Fluent shippers and basic
//! Kibana-style queries; not a full ES API surface.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use sigil_index::Indexer;

use crate::{index_json_doc, source_object, ApiError};

/// `POST /_bulk` — newline-delimited action/source pairs.
pub(crate) async fn bulk(
    State(indexer): State<Arc<Indexer>>,
    body: String,
) -> Result<Json<Value>, ApiError> {
    let mut items = Vec::new();
    let mut errors = false;
    let mut lines = body
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_string)
        .peekable();

    while let Some(action_line) = lines.next() {
        let action: Value = serde_json::from_str(&action_line)
            .map_err(|e| anyhow::anyhow!("bulk action line: {e}"))?;
        let Some((op, meta)) = action.as_object().and_then(|o| o.iter().next()) else {
            errors = true;
            continue;
        };
        let index = meta
            .get("_index")
            .and_then(Value::as_str)
            .unwrap_or("bulk")
            .to_string();
        let id = meta
            .get("_id")
            .and_then(Value::as_str)
            .map(str::to_string);

        if op == "delete" {
            // Deletes are accepted but not applied (append-only store, Phase 4).
            items.push(json!({ "delete": { "_index": index, "_id": id, "status": 200 } }));
            continue;
        }

        let Some(doc_line) = lines.next() else {
            errors = true;
            break;
        };
        // `update` wraps the document under `doc`.
        let doc_bytes = if op == "update" {
            match serde_json::from_str::<Value>(&doc_line) {
                Ok(v) => v
                    .get("doc")
                    .cloned()
                    .unwrap_or(v)
                    .to_string()
                    .into_bytes(),
                Err(_) => doc_line.into_bytes(),
            }
        } else {
            doc_line.into_bytes()
        };

        match index_json_doc(&indexer, &index, &doc_bytes) {
            Ok(()) => items.push(json!({
                op.clone(): { "_index": index, "_id": id, "status": 201, "result": "created" }
            })),
            Err(e) => {
                errors = true;
                items.push(json!({
                    op.clone(): { "_index": index, "_id": id, "status": 400, "error": e.to_string() }
                }));
            }
        }
    }

    indexer.commit()?;
    Ok(Json(json!({ "took": 0, "errors": errors, "items": items })))
}

#[derive(Debug, Deserialize)]
pub(crate) struct EsSearchParams {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    size: Option<usize>,
}

/// `GET /_search?q=...&size=...`
pub(crate) async fn search_get(
    State(indexer): State<Arc<Indexer>>,
    Query(params): Query<EsSearchParams>,
) -> Result<Json<Value>, ApiError> {
    run_es_search(&indexer, &params.q.unwrap_or_default(), params.size.unwrap_or(10))
}

/// `POST /_search` with an (abbreviated) ES query DSL body.
pub(crate) async fn search_post(
    State(indexer): State<Arc<Indexer>>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let size = body.get("size").and_then(Value::as_u64).unwrap_or(10) as usize;
    let query = parse_query_dsl(&body);
    run_es_search(&indexer, &query, size)
}

/// Translate the supported subset of the ES query DSL to a query string.
/// `match_all` → empty (everything); `query_string.query` / `match.<f>` → text.
fn parse_query_dsl(body: &Value) -> String {
    let Some(q) = body.get("query") else {
        return String::new();
    };
    if q.get("match_all").is_some() {
        return String::new();
    }
    if let Some(qs) = q.get("query_string").and_then(|x| x.get("query")).and_then(Value::as_str) {
        return qs.to_string();
    }
    if let Some(m) = q.get("match").and_then(Value::as_object) {
        if let Some((field, val)) = m.iter().next() {
            let text = val
                .get("query")
                .and_then(Value::as_str)
                .or_else(|| val.as_str())
                .unwrap_or_default();
            return format!("{field}:{text}");
        }
    }
    String::new()
}

fn run_es_search(indexer: &Indexer, query: &str, size: usize) -> Result<Json<Value>, ApiError> {
    let hits = indexer.search(query, size)?;
    let max_score = hits.iter().map(|h| h.score).fold(0.0_f32, f32::max);
    let es_hits: Vec<Value> = hits
        .iter()
        .map(|h| {
            json!({
                "_index": h.event.dataset,
                "_id": h.event.id,
                "_score": h.score,
                "_source": source_object(&h.event),
            })
        })
        .collect();
    Ok(Json(json!({
        "took": 0,
        "timed_out": false,
        "hits": {
            "total": { "value": es_hits.len(), "relation": "eq" },
            "max_score": max_score,
            "hits": es_hits,
        }
    })))
}
