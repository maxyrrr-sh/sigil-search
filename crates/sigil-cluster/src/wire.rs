//! On-the-wire encoding of [`Event`] for the transport bus.
//!
//! `sigil-core` is intentionally serde-free, so events are (de)serialized here
//! as JSON when they cross the bus between the `ingest` and `index` roles.

use serde_json::{json, Value};
use sigil_core::Event;

/// Encode an event as JSON bytes for publishing on the transport.
pub fn encode_event(event: &Event) -> Vec<u8> {
    let fields: Vec<Value> = event
        .fields
        .iter()
        .map(|(k, v)| json!([k, v]))
        .collect();
    let value = json!({
        "id": event.id,
        "ts": event.ts,
        "ingest_ts": event.ingest_ts,
        "dataset": event.dataset,
        "tenant": event.tenant,
        "template_id": event.template_id,
        "labels": event.labels,
        "fields": fields,
    });
    value.to_string().into_bytes()
}

/// Decode an event previously produced by [`encode_event`].
pub fn decode_event(bytes: &[u8]) -> anyhow::Result<Event> {
    let v: Value = serde_json::from_slice(bytes)?;
    let str_field = |key: &str| v.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    let i64_field = |key: &str| v.get(key).and_then(Value::as_i64).unwrap_or(0);

    let fields = v
        .get("fields")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|pair| {
                    let p = pair.as_array()?;
                    Some((p.first()?.as_str()?.to_string(), p.get(1)?.as_str()?.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();

    let labels = v
        .get("labels")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();

    Ok(Event {
        id: str_field("id"),
        ts: i64_field("ts"),
        ingest_ts: i64_field("ingest_ts"),
        dataset: str_field("dataset"),
        tenant: str_field("tenant"),
        fields,
        template_id: v.get("template_id").and_then(Value::as_u64),
        raw: Vec::new(),
        labels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sigil_core::ecs;

    #[test]
    fn round_trip() {
        let mut ev = Event {
            id: "abc".into(),
            ts: 123,
            ingest_ts: 456,
            dataset: "syslog".into(),
            template_id: Some(99),
            ..Default::default()
        };
        ev.set(ecs::MESSAGE, "hello world");
        ev.set(ecs::HOST_NAME, "web1");

        let bytes = encode_event(&ev);
        let back = decode_event(&bytes).unwrap();
        assert_eq!(back.id, "abc");
        assert_eq!(back.ts, 123);
        assert_eq!(back.dataset, "syslog");
        assert_eq!(back.template_id, Some(99));
        assert_eq!(back.get(ecs::MESSAGE), Some("hello world"));
        assert_eq!(back.get(ecs::HOST_NAME), Some("web1"));
    }
}
