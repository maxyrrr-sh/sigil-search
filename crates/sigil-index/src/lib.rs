//! `sigil-index` — the indexer (DESIGN §8).
//!
//! Phase 0 scope: a Tantivy-backed hot segment store. Events are indexed for
//! full-text search and stored verbatim (as JSON in a `source` field) so hits
//! can be reconstructed. Tiering (warm/cold), Parquet/DataFusion analytics and
//! the catalog land in Phase 2.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sigil_core::{ecs, Event};
use tantivy::collector::TopDocs;
use tantivy::query::{AllQuery, QueryParser};
use tantivy::schema::{Field, OwnedValue, Schema, STORED, STRING, TEXT};
use tantivy::{Index, IndexReader, IndexWriter, TantivyDocument};

mod tier;
pub use tier::{Catalog, Segment};

/// A stored, reconstructable copy of an indexed event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedEvent {
    pub id: String,
    pub ts: i64,
    pub ingest_ts: i64,
    pub dataset: String,
    pub fields: Vec<(String, String)>,
}

impl IndexedEvent {
    fn from_event(id: &str, ev: &Event) -> Self {
        IndexedEvent {
            id: id.to_string(),
            ts: ev.ts,
            ingest_ts: ev.ingest_ts,
            dataset: ev.dataset.clone(),
            fields: ev.fields.clone(),
        }
    }
}

/// A single search result.
#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub score: f32,
    #[serde(flatten)]
    pub event: IndexedEvent,
}

struct Fields {
    id: Field,
    message: Field,
    host: Field,
    dataset: Field,
    all: Field,
    source: Field,
}

/// A Tantivy index over normalized [`Event`]s, plus a cold-segment catalog.
/// Cheap to clone-share via `Arc`.
pub struct Indexer {
    index: Index,
    reader: IndexReader,
    writer: Mutex<IndexWriter>,
    fields: Fields,
    counter: AtomicU64,
    /// Index directory (None for in-memory indexes; disables tiering).
    dir: Option<PathBuf>,
    catalog: Mutex<Catalog>,
}

impl Indexer {
    fn build_schema() -> (Schema, Fields) {
        let mut sb = Schema::builder();
        let fields = Fields {
            id: sb.add_text_field("id", STRING | STORED),
            message: sb.add_text_field("message", TEXT),
            host: sb.add_text_field("host", TEXT),
            dataset: sb.add_text_field("dataset", STRING | STORED),
            all: sb.add_text_field("all", TEXT),
            source: sb.add_text_field("source", STORED),
        };
        (sb.build(), fields)
    }

    /// Open (or create) an on-disk index at `dir`.
    pub fn open(dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let (schema, fields) = Self::build_schema();
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        let mmap = tantivy::directory::MmapDirectory::open(&dir)?;
        let index = Index::open_or_create(mmap, schema)?;
        Self::from_index(index, fields, Some(dir))
    }

    /// Create an ephemeral in-memory index (used by tests). Tiering is disabled.
    pub fn in_memory() -> anyhow::Result<Self> {
        let (schema, fields) = Self::build_schema();
        let index = Index::create_in_ram(schema);
        Self::from_index(index, fields, None)
    }

    fn from_index(index: Index, fields: Fields, dir: Option<PathBuf>) -> anyhow::Result<Self> {
        let writer: IndexWriter = index.writer(50_000_000)?;
        let reader = index.reader()?;
        let catalog = match &dir {
            Some(d) => Catalog::load(&d.join("catalog.json")),
            None => Catalog::default(),
        };
        Ok(Indexer {
            index,
            reader,
            writer: Mutex::new(writer),
            fields,
            counter: AtomicU64::new(0),
            dir,
            catalog: Mutex::new(catalog),
        })
    }

    // --- tiering (DESIGN §8) ---------------------------------------------

    /// Paths of existing cold Parquet segments, for the query engine to union.
    pub fn cold_paths(&self) -> Vec<PathBuf> {
        self.catalog
            .lock()
            .expect("catalog poisoned")
            .segments
            .iter()
            .map(|s| PathBuf::from(&s.path))
            .filter(|p| p.exists())
            .collect()
    }

    /// Current cold segments (metadata).
    pub fn segments(&self) -> Vec<Segment> {
        self.catalog.lock().expect("catalog poisoned").segments.clone()
    }

    /// Roll the hot tier to a new cold Parquet segment, then clear hot.
    /// Returns the new segment, or `None` if there was nothing to roll.
    pub fn archive(&self) -> anyhow::Result<Option<Segment>> {
        let Some(dir) = self.dir.clone() else {
            anyhow::bail!("archive requires a persistent (on-disk) index");
        };
        let events = self.dump(100_000_000)?;
        if events.is_empty() {
            return Ok(None);
        }
        let (min_ts, max_ts) = events.iter().fold((i64::MAX, i64::MIN), |(lo, hi), e| {
            (lo.min(e.ts), hi.max(e.ts))
        });

        let mut catalog = self.catalog.lock().expect("catalog poisoned");
        let id = catalog.next_id;
        let cold_dir = dir.join("cold");
        std::fs::create_dir_all(&cold_dir)?;
        let path = cold_dir.join(format!("seg-{id:08}.parquet"));
        tier::write_parquet(&path, &events)?;

        // Clear the hot tier now that the rows are durable in cold.
        {
            let mut writer = self.writer.lock().expect("index writer poisoned");
            writer.delete_all_documents()?;
            writer.commit()?;
        }
        self.reader.reload()?;

        let segment = Segment {
            id,
            path: path.to_string_lossy().into_owned(),
            min_ts,
            max_ts,
            count: events.len(),
            created_ts: now_micros(),
        };
        catalog.segments.push(segment.clone());
        catalog.next_id += 1;
        catalog.save(&dir.join("catalog.json"))?;
        Ok(Some(segment))
    }

    /// Delete cold segments older than `max_age_secs`. Returns how many were removed.
    pub fn prune_cold(&self, max_age_secs: u64) -> anyhow::Result<usize> {
        let Some(dir) = self.dir.clone() else {
            return Ok(0);
        };
        let cutoff = now_micros() - (max_age_secs as i64) * 1_000_000;
        let mut catalog = self.catalog.lock().expect("catalog poisoned");
        let before = catalog.segments.len();
        let mut kept = Vec::with_capacity(before);
        for seg in std::mem::take(&mut catalog.segments) {
            if seg.created_ts < cutoff {
                let _ = std::fs::remove_file(&seg.path);
            } else {
                kept.push(seg);
            }
        }
        catalog.segments = kept;
        catalog.save(&dir.join("catalog.json"))?;
        Ok(before - catalog.segments.len())
    }

    /// Index one normalized event. Call [`Indexer::commit`] to make it searchable.
    pub fn index(&self, event: &Event) -> anyhow::Result<()> {
        let id = if event.id.is_empty() {
            format!(
                "{}-{}",
                event.ingest_ts,
                self.counter.fetch_add(1, Ordering::Relaxed)
            )
        } else {
            event.id.clone()
        };

        let source = serde_json::to_string(&IndexedEvent::from_event(&id, event))?;
        let all: String = event
            .fields
            .iter()
            .map(|(_, v)| v.as_str())
            .collect::<Vec<_>>()
            .join(" ");

        let mut doc = TantivyDocument::default();
        doc.add_text(self.fields.id, &id);
        doc.add_text(self.fields.source, &source);
        doc.add_text(self.fields.all, &all);
        doc.add_text(self.fields.dataset, &event.dataset);
        if let Some(m) = event.get(ecs::MESSAGE) {
            doc.add_text(self.fields.message, m);
        }
        if let Some(h) = event.get(ecs::HOST_NAME) {
            doc.add_text(self.fields.host, h);
        }

        self.writer
            .lock()
            .expect("index writer poisoned")
            .add_document(doc)?;
        Ok(())
    }

    /// Commit buffered documents and refresh the reader so they become visible.
    pub fn commit(&self) -> anyhow::Result<()> {
        self.writer.lock().expect("index writer poisoned").commit()?;
        self.reader.reload()?;
        Ok(())
    }

    /// Return up to `limit` stored events (match-all), for analytics/export.
    pub fn dump(&self, limit: usize) -> anyhow::Result<Vec<IndexedEvent>> {
        Ok(self.search("", limit)?.into_iter().map(|h| h.event).collect())
    }

    /// Full-text search over `message`, `host`, and the catch-all field.
    /// An empty query returns the most recent documents (match-all).
    pub fn search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<Hit>> {
        let searcher = self.reader.searcher();
        let collector = TopDocs::with_limit(limit.max(1));

        let results = if query.trim().is_empty() {
            searcher.search(&AllQuery, &collector)?
        } else {
            let parser = QueryParser::for_index(
                &self.index,
                vec![self.fields.message, self.fields.all, self.fields.host],
            );
            let parsed = parser.parse_query(query)?;
            searcher.search(&parsed, &collector)?
        };

        let mut hits = Vec::with_capacity(results.len());
        for (score, addr) in results {
            let doc: TantivyDocument = searcher.doc(addr)?;
            if let Some(OwnedValue::Str(json)) = doc.get_first(self.fields.source) {
                let event: IndexedEvent = serde_json::from_str(json)?;
                hits.push(Hit { score, event });
            }
        }
        Ok(hits)
    }
}

fn now_micros() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(dataset: &str, message: &str, host: &str) -> Event {
        let mut ev = Event {
            dataset: dataset.to_string(),
            ..Default::default()
        };
        ev.set(ecs::MESSAGE, message);
        ev.set(ecs::HOST_NAME, host);
        ev
    }

    #[test]
    fn index_and_search_roundtrip() {
        let idx = Indexer::in_memory().unwrap();
        idx.index(&event("syslog_main", "failed login for root", "web1"))
            .unwrap();
        idx.index(&event("syslog_main", "accepted password for alice", "web2"))
            .unwrap();
        idx.commit().unwrap();

        let hits = idx.search("login", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].event.dataset, "syslog_main");

        // Empty query is match-all.
        assert_eq!(idx.search("", 10).unwrap().len(), 2);
        // Field-scoped term query against the host field.
        assert_eq!(idx.search("host:web2", 10).unwrap().len(), 1);
    }

    #[test]
    fn archive_rolls_hot_to_cold() {
        let dir = std::env::temp_dir().join(format!("sigil-idx-test-{}", now_micros()));
        let idx = Indexer::open(&dir).unwrap();
        idx.index(&event("syslog", "alpha", "h1")).unwrap();
        idx.index(&event("syslog", "beta", "h2")).unwrap();
        idx.commit().unwrap();
        assert_eq!(idx.search("", 10).unwrap().len(), 2);

        let seg = idx.archive().unwrap().expect("a segment");
        assert_eq!(seg.count, 2);
        // Hot is now empty; the rows live in a cold segment.
        assert_eq!(idx.search("", 10).unwrap().len(), 0);
        assert_eq!(idx.cold_paths().len(), 1);
        assert!(idx.cold_paths()[0].exists());

        // Nothing left to roll.
        assert!(idx.archive().unwrap().is_none());

        std::fs::remove_dir_all(&dir).ok();
    }
}
