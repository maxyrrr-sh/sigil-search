//! OTLP/HTTP logs receiver (DESIGN §10, Phase 4). Accepts the OTLP **JSON**
//! encoding at `POST /v1/logs` (the protobuf/gRPC encoding is a later addition).

use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};
use sigil_core::Record;
use sigil_index::Indexer;
use sigil_schema::{EcsSchema, Schema};

use crate::ApiError;

/// `POST /v1/logs` — OTLP ExportLogsServiceRequest (JSON).
pub(crate) async fn logs(
    State(indexer): State<Arc<Indexer>>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let schema = EcsSchema::new("otlp");
    let mut accepted = 0;
    let mut rejected = 0;

    for resource_logs in array(&body, "resourceLogs") {
        let resource_attrs =
            collect_attributes(resource_logs.get("resource").and_then(|r| r.get("attributes")));
        for scope_logs in array(resource_logs, "scopeLogs") {
            for record in array(scope_logs, "logRecords") {
                let mut rec = Record::default();
                for (k, v) in &resource_attrs {
                    rec.push(k.clone(), v.clone());
                }
                if let Some(msg) = record
                    .get("body")
                    .and_then(|b| b.get("stringValue"))
                    .and_then(Value::as_str)
                {
                    rec.push("message", msg.to_string());
                }
                if let Some(sev) = record.get("severityText").and_then(Value::as_str) {
                    rec.push("log.level", sev.to_string());
                }
                if let Some(ts) = record.get("timeUnixNano").and_then(scalar_string) {
                    rec.push("timestamp", ts);
                }
                for (k, v) in collect_attributes(record.get("attributes")) {
                    rec.push(k, v);
                }
                match schema.normalize(rec) {
                    Ok(event) => match indexer.index(&event) {
                        Ok(()) => accepted += 1,
                        Err(_) => rejected += 1,
                    },
                    Err(_) => rejected += 1,
                }
            }
        }
    }

    indexer.commit()?;
    Ok(Json(json!({
        "partialSuccess": {
            "rejectedLogRecords": rejected,
            "acceptedLogRecords": accepted,
        }
    })))
}

fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// Flatten OTLP `attributes` (`[{key, value:{stringValue|intValue|...}}]`).
fn collect_attributes(attrs: Option<&Value>) -> Vec<(String, String)> {
    let Some(list) = attrs.and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|a| {
            let key = a.get("key").and_then(Value::as_str)?.to_string();
            let value = a.get("value").and_then(otlp_any_value)?;
            Some((key, value))
        })
        .collect()
}

fn otlp_any_value(value: &Value) -> Option<String> {
    for k in ["stringValue", "intValue", "doubleValue", "boolValue"] {
        if let Some(v) = value.get(k) {
            return scalar_string(v);
        }
    }
    None
}

fn scalar_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}
