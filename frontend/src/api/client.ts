// Thin fetch client for the Sigil HTTP API. In dev, calls go to `/api/*` which
// Vite proxies to the backend (see vite.config.ts). Override with VITE_API_BASE.
import type { RowsResponse, SearchResponse } from './types';

const BASE: string = (import.meta as any).env?.VITE_API_BASE || '/api';

async function getJSON<T>(path: string): Promise<T> {
  const sep = path.includes('?') ? '&' : '?';
  const res = await fetch(`${BASE}${path}${sep}_=${Date.now()}`, { cache: 'no-store' });
  if (!res.ok) {
    let message = `${res.status} ${res.statusText}`;
    try {
      const body = await res.json();
      if (body && body.error) message = body.error;
    } catch {
      /* non-JSON error body */
    }
    throw new Error(message);
  }
  return res.json() as Promise<T>;
}

export async function health(): Promise<boolean> {
  try {
    const res = await fetch(`${BASE}/healthz`, { cache: 'no-store' });
    return res.ok;
  } catch {
    return false;
  }
}

export function searchLucene(q: string, limit = 500): Promise<SearchResponse> {
  return getJSON<SearchResponse>(`/search?q=${encodeURIComponent(q)}&limit=${limit}`);
}

export function runDsl(q: string): Promise<RowsResponse> {
  return getJSON<RowsResponse>(`/query?q=${encodeURIComponent(q)}`);
}

export function runSql(q: string): Promise<RowsResponse> {
  return getJSON<RowsResponse>(`/sql?q=${encodeURIComponent(q)}`);
}
