// Wire types for the Sigil HTTP API + the normalized shapes the UI renders.

export type Mode = 'dsl' | 'lucene' | 'sql';

/** `GET /search` hit (Lucene query-string). */
export interface Hit {
  score: number;
  id: string;
  ts: number; // epoch microseconds
  ingest_ts: number;
  dataset: string;
  fields: [string, string][];
}

export interface SearchResponse {
  query: string;
  count: number;
  hits: Hit[];
}

/** `GET /query` (pipe-DSL) and `GET /sql` response. */
export interface RowsResponse {
  count: number;
  rows: Record<string, unknown>[];
  dsl?: string;
  sql?: string;
  error?: string;
}

/** A row in the events (Discover) view, normalized across all three modes. */
export interface EventRow {
  ts: number | null; // epoch microseconds
  fields: Record<string, string>;
  raw: Record<string, unknown>;
  score?: number;
  id?: string;
  dataset?: string;
}

export interface QueryResult {
  mode: Mode;
  kind: 'events' | 'stats';
  events: EventRow[];
  rows: Record<string, unknown>[];
  columns: string[];
  sql?: string; // lowered SQL (dsl) or executed SQL
  total: number;
}

export interface TimeRange {
  from: string; // datemath, e.g. "now-15m"
  to: string; // datemath, e.g. "now"
}

export interface HistogramBucket {
  t: number; // bucket start, epoch milliseconds
  count: number;
}
