// Orchestrates a search across the three query modes and normalizes results.
import dateMath from '@elastic/datemath';

import { runDsl, runSql, searchLucene } from '../api/client';
import type {
  EventRow,
  HistogramBucket,
  Hit,
  Mode,
  QueryResult,
  RowsResponse,
  TimeRange,
} from '../api/types';
import { fieldsToRecord } from './format';

export function resolveRangeMicros(range: TimeRange): { fromMicros: number; toMicros: number } {
  const from = dateMath.parse(range.from);
  const to = dateMath.parse(range.to, { roundUp: true });
  const fromMs = from ? from.valueOf() : Date.now() - 15 * 60 * 1000;
  const toMs = to ? to.valueOf() : Date.now();
  return { fromMicros: fromMs * 1000, toMicros: toMs * 1000 };
}

function hitToEventRow(hit: Hit): EventRow {
  return {
    ts: hit.ts,
    fields: fieldsToRecord(hit.fields),
    raw: hit as unknown as Record<string, unknown>,
    score: hit.score,
    id: hit.id,
    dataset: hit.dataset,
  };
}

function rowToEventRow(row: Record<string, unknown>): EventRow {
  const fields: Record<string, string> = {};
  for (const [k, v] of Object.entries(row)) {
    if (v == null) continue;
    fields[k] = typeof v === 'string' ? v : JSON.stringify(v);
  }
  const ts = typeof row.ts === 'number' ? row.ts : Number(row.ts);
  return {
    ts: Number.isFinite(ts) ? ts : null,
    fields,
    raw: row,
    id: row.id != null ? String(row.id) : undefined,
    dataset: row.dataset != null ? String(row.dataset) : undefined,
  };
}

// SQL/DSL rows that carry `ts` + `message` are events; anything else (a
// `stats count by ...` / GROUP BY) is an aggregation table.
function rowsAreEvents(rows: Record<string, unknown>[]): boolean {
  if (rows.length === 0) return true;
  return 'ts' in rows[0] && 'message' in rows[0];
}

function filterByRange(events: EventRow[], range: TimeRange): EventRow[] {
  const { fromMicros, toMicros } = resolveRangeMicros(range);
  return events.filter((e) => e.ts == null || (e.ts >= fromMicros && e.ts <= toMicros));
}

function rowsToResult(mode: Mode, res: RowsResponse, range: TimeRange): QueryResult {
  const rows = res.rows || [];
  if (rowsAreEvents(rows)) {
    const events = filterByRange(rows.map(rowToEventRow), range);
    return { mode, kind: 'events', events, rows: [], columns: [], sql: res.sql, total: events.length };
  }
  const columns = rows.length ? Object.keys(rows[0]) : [];
  return { mode, kind: 'stats', events: [], rows, columns, sql: res.sql, total: rows.length };
}

export async function runSearch(mode: Mode, text: string, range: TimeRange): Promise<QueryResult> {
  if (mode === 'lucene') {
    const res = await searchLucene(text || '', 500);
    const events = filterByRange(res.hits.map(hitToEventRow), range);
    return { mode, kind: 'events', events, rows: [], columns: [], total: events.length };
  }
  if (mode === 'dsl') {
    return rowsToResult('dsl', await runDsl(text), range);
  }
  return rowsToResult('sql', await runSql(text), range);
}

/** Bucket events over the selected time range for the Discover histogram. */
export function buildHistogram(events: EventRow[], range: TimeRange, buckets = 48): HistogramBucket[] {
  const { fromMicros, toMicros } = resolveRangeMicros(range);
  const span = Math.max(1, toMicros - fromMicros);
  const size = span / buckets;
  const out: HistogramBucket[] = [];
  for (let i = 0; i < buckets; i++) out.push({ t: Math.floor((fromMicros + i * size) / 1000), count: 0 });
  for (const e of events) {
    if (e.ts == null) continue;
    let idx = Math.floor((e.ts - fromMicros) / size);
    if (idx < 0) idx = 0;
    if (idx >= buckets) idx = buckets - 1;
    out[idx].count++;
  }
  return out;
}
