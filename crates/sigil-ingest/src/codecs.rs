//! Codecs: decode raw input bytes into [`Record`]s (DESIGN §6).

use std::collections::HashMap;

use regex::Regex;
use sigil_core::{Codec, Plugin, PluginManifest, Record, Result};

use crate::manifest;

// ---------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------

/// Decode JSON (one object per call, or a JSON array of objects). Nested
/// objects/arrays flatten to dotted keys; scalars are stringified.
pub struct JsonCodec {
    manifest: PluginManifest,
}

impl Default for JsonCodec {
    fn default() -> Self {
        JsonCodec {
            manifest: manifest("json", "decode"),
        }
    }
}

impl Plugin for JsonCodec {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
}

impl Codec for JsonCodec {
    fn decode(&self, raw: &[u8]) -> Result<Vec<Record>> {
        let value: serde_json::Value = serde_json::from_slice(raw)
            .map_err(|e| sigil_core::Error(format!("json decode: {e}")))?;
        let mut records = Vec::new();
        match value {
            serde_json::Value::Array(items) => {
                for item in items {
                    records.push(record_from_json(item));
                }
            }
            other => records.push(record_from_json(other)),
        }
        Ok(records)
    }
}

/// Flatten a single JSON value into a [`Record`] (public for the ES `_bulk` path).
pub fn record_from_json(value: serde_json::Value) -> Record {
    let mut rec = Record::default();
    flatten_json("", &value, &mut rec);
    rec
}

fn flatten_json(prefix: &str, value: &serde_json::Value, rec: &mut Record) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten_json(&key, v, rec);
            }
        }
        serde_json::Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                let key = format!("{prefix}.{i}");
                flatten_json(&key, v, rec);
            }
        }
        serde_json::Value::String(s) => rec.push(prefix, s.clone()),
        serde_json::Value::Null => {}
        scalar => rec.push(prefix, scalar.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Syslog (RFC 3164 / RFC 5424, best effort)
// ---------------------------------------------------------------------------

pub struct SyslogCodec {
    manifest: PluginManifest,
}

impl Default for SyslogCodec {
    fn default() -> Self {
        SyslogCodec {
            manifest: manifest("syslog", "decode"),
        }
    }
}

impl Plugin for SyslogCodec {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
}

impl Codec for SyslogCodec {
    fn decode(&self, raw: &[u8]) -> Result<Vec<Record>> {
        let line = String::from_utf8_lossy(raw);
        Ok(vec![parse_syslog(line.trim_end())])
    }
}

fn parse_syslog(line: &str) -> Record {
    let mut rec = Record::default();
    let mut rest = line;
    if let Some(stripped) = rest.strip_prefix('<') {
        if let Some(end) = stripped.find('>') {
            if let Ok(pri) = stripped[..end].parse::<u32>() {
                rec.push("priority", pri.to_string());
                rec.push("facility", (pri / 8).to_string());
                rec.push("severity", (pri % 8).to_string());
                rest = &stripped[end + 1..];
            }
        }
    }
    if let Some(after_ver) = rest.strip_prefix("1 ") {
        parse_rfc5424(after_ver, &mut rec);
    } else {
        parse_rfc3164(rest, &mut rec);
    }
    rec
}

fn parse_rfc5424(s: &str, rec: &mut Record) {
    let mut parts = s.splitn(6, ' ');
    let mut take = |key: &str, rec: &mut Record| {
        if let Some(tok) = parts.next() {
            if tok != "-" && !tok.is_empty() {
                rec.push(key, tok.to_string());
            }
        }
    };
    take("timestamp", rec);
    take("host", rec);
    take("app", rec);
    take("pid", rec);
    take("msgid", rec);
    if let Some(remainder) = parts.next() {
        rec.push("message", strip_structured_data(remainder).trim().to_string());
    }
}

fn strip_structured_data(s: &str) -> &str {
    let s = s.trim_start();
    if let Some(rest) = s.strip_prefix('-') {
        return rest;
    }
    if s.starts_with('[') {
        if let Some(close) = s.find(']') {
            return &s[close + 1..];
        }
    }
    s
}

fn parse_rfc3164(s: &str, rec: &mut Record) {
    let tokens: Vec<&str> = s.splitn(5, ' ').collect();
    if tokens.len() >= 5 && is_month(tokens[0]) {
        rec.push("timestamp", format!("{} {} {}", tokens[0], tokens[1], tokens[2]));
        rec.push("host", tokens[3].to_string());
        parse_tag_and_message(tokens[4], rec);
    } else {
        rec.push("message", s.to_string());
    }
}

fn parse_tag_and_message(s: &str, rec: &mut Record) {
    if let Some(colon) = s.find(": ") {
        let (tag, msg) = s.split_at(colon);
        let msg = &msg[2..];
        if let Some(open) = tag.find('[') {
            rec.push("app", tag[..open].to_string());
            if let Some(close) = tag.find(']') {
                rec.push("pid", tag[open + 1..close].to_string());
            }
        } else {
            rec.push("app", tag.to_string());
        }
        rec.push("message", msg.to_string());
    } else {
        rec.push("message", s.to_string());
    }
}

fn is_month(tok: &str) -> bool {
    matches!(
        tok,
        "Jan" | "Feb" | "Mar" | "Apr" | "May" | "Jun" | "Jul" | "Aug" | "Sep" | "Oct" | "Nov"
            | "Dec"
    )
}

// ---------------------------------------------------------------------------
// key=value
// ---------------------------------------------------------------------------

/// Decode `key=value key2="quoted value"` lines.
pub struct KvCodec {
    manifest: PluginManifest,
}

impl Default for KvCodec {
    fn default() -> Self {
        KvCodec {
            manifest: manifest("kv", "decode"),
        }
    }
}

impl Plugin for KvCodec {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
}

impl Codec for KvCodec {
    fn decode(&self, raw: &[u8]) -> Result<Vec<Record>> {
        let line = String::from_utf8_lossy(raw);
        Ok(vec![parse_kv(&line)])
    }
}

fn parse_kv(line: &str) -> Record {
    let mut rec = Record::default();
    for (k, v) in split_kv(line) {
        rec.push(k, v);
    }
    rec
}

/// Split a line into key/value pairs honoring double-quoted values.
fn split_kv(line: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let key_start = i;
        while i < bytes.len() && bytes[i] != b'=' && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            // Token without '=' — skip to next whitespace.
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            continue;
        }
        let key = line[key_start..i].to_string();
        i += 1; // skip '='
        let value = if i < bytes.len() && bytes[i] == b'"' {
            i += 1;
            let val_start = i;
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            let v = line[val_start..i].to_string();
            if i < bytes.len() {
                i += 1; // skip closing quote
            }
            v
        } else {
            let val_start = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            line[val_start..i].to_string()
        };
        if !key.is_empty() {
            pairs.push((key, value));
        }
    }
    pairs
}

// ---------------------------------------------------------------------------
// CSV (RFC 4180-ish, with configured headers)
// ---------------------------------------------------------------------------

/// Decode CSV rows into records, naming columns from `headers` (or `col0..N`).
pub struct CsvCodec {
    headers: Vec<String>,
    manifest: PluginManifest,
}

impl CsvCodec {
    pub fn new(headers: Vec<String>) -> Self {
        CsvCodec {
            headers,
            manifest: manifest("csv", "decode"),
        }
    }
}

impl Default for CsvCodec {
    fn default() -> Self {
        CsvCodec::new(Vec::new())
    }
}

impl Plugin for CsvCodec {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
}

impl Codec for CsvCodec {
    fn decode(&self, raw: &[u8]) -> Result<Vec<Record>> {
        let text = String::from_utf8_lossy(raw);
        let mut records = Vec::new();
        for line in text.lines() {
            if line.is_empty() {
                continue;
            }
            let cols = parse_csv_line(line);
            let mut rec = Record::default();
            for (i, col) in cols.into_iter().enumerate() {
                let key = self
                    .headers
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| format!("col{i}"));
                rec.push(key, col);
            }
            records.push(rec);
        }
        Ok(records)
    }
}

fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if in_quotes => {
                if chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            }
            '"' => in_quotes = true,
            ',' if !in_quotes => {
                fields.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    fields.push(cur);
    fields
}

// ---------------------------------------------------------------------------
// Regex (named captures)
// ---------------------------------------------------------------------------

/// Decode a line via a regex; each named capture becomes a field.
pub struct RegexCodec {
    re: Regex,
    manifest: PluginManifest,
}

impl RegexCodec {
    pub fn new(pattern: &str) -> Result<Self> {
        let re = Regex::new(pattern)
            .map_err(|e| sigil_core::Error(format!("regex codec: {e}")))?;
        Ok(RegexCodec {
            re,
            manifest: manifest("regex", "decode"),
        })
    }
}

impl Plugin for RegexCodec {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
}

impl Codec for RegexCodec {
    fn decode(&self, raw: &[u8]) -> Result<Vec<Record>> {
        let line = String::from_utf8_lossy(raw);
        let mut rec = Record::default();
        if let Some(caps) = self.re.captures(line.trim_end()) {
            for name in self.re.capture_names().flatten() {
                if let Some(m) = caps.name(name) {
                    rec.push(name, m.as_str().to_string());
                }
            }
        }
        Ok(vec![rec])
    }
}

// ---------------------------------------------------------------------------
// Grok (%{PATTERN:name}) over a small builtin pattern library
// ---------------------------------------------------------------------------

/// Decode a line with a grok expression, compiled down to a regex.
pub struct GrokCodec {
    re: Regex,
    manifest: PluginManifest,
}

impl GrokCodec {
    pub fn new(expr: &str) -> Result<Self> {
        let pattern = compile_grok(expr)?;
        let re = Regex::new(&pattern)
            .map_err(|e| sigil_core::Error(format!("grok compile -> {pattern}: {e}")))?;
        Ok(GrokCodec {
            re,
            manifest: manifest("grok", "decode"),
        })
    }
}

impl Plugin for GrokCodec {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
}

impl Codec for GrokCodec {
    fn decode(&self, raw: &[u8]) -> Result<Vec<Record>> {
        let line = String::from_utf8_lossy(raw);
        let mut rec = Record::default();
        if let Some(caps) = self.re.captures(line.trim_end()) {
            for name in self.re.capture_names().flatten() {
                if let Some(m) = caps.name(name) {
                    rec.push(name, m.as_str().to_string());
                }
            }
        }
        Ok(vec![rec])
    }
}

fn grok_patterns() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        ("INT", r"[+-]?\d+"),
        ("NUMBER", r"[+-]?\d+(?:\.\d+)?"),
        ("WORD", r"\w+"),
        ("NOTSPACE", r"\S+"),
        ("SPACE", r"\s*"),
        ("DATA", r".*?"),
        ("GREEDYDATA", r".*"),
        ("IP", r"\d{1,3}(?:\.\d{1,3}){3}"),
        ("IPV4", r"\d{1,3}(?:\.\d{1,3}){3}"),
        ("HOSTNAME", r"[A-Za-z0-9_.-]+"),
        ("USER", r"[A-Za-z0-9._-]+"),
        ("LOGLEVEL", r"(?i:TRACE|DEBUG|INFO|WARN|WARNING|ERROR|FATAL)"),
        ("TIMESTAMP_ISO8601", r"\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?"),
    ])
}

/// Expand `%{PATTERN}` and `%{PATTERN:name}` into a plain regex string.
fn compile_grok(expr: &str) -> Result<String> {
    let token = Regex::new(r"%\{(\w+)(?::(\w+))?\}").expect("static grok token regex");
    let patterns = grok_patterns();
    let mut out = String::new();
    let mut last = 0;
    for caps in token.captures_iter(expr) {
        let m = caps.get(0).unwrap();
        // Escape the literal text between tokens.
        out.push_str(&regex::escape(&expr[last..m.start()]));
        last = m.end();
        let name = caps.get(1).unwrap().as_str();
        let sub = patterns
            .get(name)
            .ok_or_else(|| sigil_core::Error(format!("grok: unknown pattern %{{{name}}}")))?;
        match caps.get(2) {
            Some(bind) => out.push_str(&format!("(?P<{}>{})", bind.as_str(), sub)),
            None => out.push_str(&format!("(?:{sub})")),
        }
    }
    out.push_str(&regex::escape(&expr[last..]));
    Ok(out)
}

// ---------------------------------------------------------------------------
// CEF (ArcSight Common Event Format)
// ---------------------------------------------------------------------------

/// Decode `CEF:0|Vendor|Product|Version|SigID|Name|Severity|ext=...`.
pub struct CefCodec {
    manifest: PluginManifest,
}

impl Default for CefCodec {
    fn default() -> Self {
        CefCodec {
            manifest: manifest("cef", "decode"),
        }
    }
}

impl Plugin for CefCodec {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
}

impl Codec for CefCodec {
    fn decode(&self, raw: &[u8]) -> Result<Vec<Record>> {
        let line = String::from_utf8_lossy(raw);
        let body = line
            .trim()
            .strip_prefix("CEF:")
            .ok_or_else(|| sigil_core::Error("cef: missing 'CEF:' prefix".into()))?;
        // The header has exactly 7 pipe-delimited fields; the 7th boundary ends
        // the header and the remainder is the key=value extension.
        let header: Vec<&str> = body.splitn(8, '|').collect();
        if header.len() < 8 {
            return Err(sigil_core::Error("cef: too few header fields".into()));
        }
        let mut rec = Record::default();
        rec.push("cef.version", header[0].to_string());
        rec.push("cef.device_vendor", header[1].to_string());
        rec.push("cef.device_product", header[2].to_string());
        rec.push("cef.device_version", header[3].to_string());
        rec.push("cef.signature_id", header[4].to_string());
        rec.push("cef.name", header[5].to_string());
        rec.push("cef.severity", header[6].to_string());
        rec.push("message", header[5].to_string());
        for (k, v) in split_kv(header[7]) {
            rec.push(format!("cef.ext.{k}"), v);
        }
        Ok(vec![rec])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kv_with_quotes() {
        let r = &KvCodec::default()
            .decode(br#"src=10.0.0.1 user="alice smith" action=login"#)
            .unwrap()[0];
        assert_eq!(r.get("src"), Some("10.0.0.1"));
        assert_eq!(r.get("user"), Some("alice smith"));
        assert_eq!(r.get("action"), Some("login"));
    }

    #[test]
    fn csv_with_headers() {
        let codec = CsvCodec::new(vec!["ts".into(), "host".into(), "msg".into()]);
        let r = &codec
            .decode(b"2026-01-01,web1,\"hello, world\"")
            .unwrap()[0];
        assert_eq!(r.get("host"), Some("web1"));
        assert_eq!(r.get("msg"), Some("hello, world"));
    }

    #[test]
    fn regex_named_captures() {
        let codec = RegexCodec::new(r"(?P<method>\w+)\s+(?P<path>/\S*)").unwrap();
        let r = &codec.decode(b"GET /index.html HTTP/1.1").unwrap()[0];
        assert_eq!(r.get("method"), Some("GET"));
        assert_eq!(r.get("path"), Some("/index.html"));
    }

    #[test]
    fn grok_apache_like() {
        let codec = GrokCodec::new("%{IP:client} %{WORD:method} %{NUMBER:status}").unwrap();
        let r = &codec.decode(b"10.0.0.9 GET 200").unwrap()[0];
        assert_eq!(r.get("client"), Some("10.0.0.9"));
        assert_eq!(r.get("method"), Some("GET"));
        assert_eq!(r.get("status"), Some("200"));
    }

    #[test]
    fn cef_header_and_extension() {
        let line = b"CEF:0|Acme|Box|1.0|100|User Login|5|src=10.0.0.1 suser=alice";
        let r = &CefCodec::default().decode(line).unwrap()[0];
        assert_eq!(r.get("cef.device_vendor"), Some("Acme"));
        assert_eq!(r.get("cef.name"), Some("User Login"));
        assert_eq!(r.get("cef.severity"), Some("5"));
        assert_eq!(r.get("cef.ext.src"), Some("10.0.0.1"));
        assert_eq!(r.get("cef.ext.suser"), Some("alice"));
    }
}
