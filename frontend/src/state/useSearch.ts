import { useCallback, useState } from 'react';

import type { Mode, QueryResult, TimeRange } from '../api/types';
import { runSearch } from '../lib/query';

export function useSearch() {
  const [result, setResult] = useState<QueryResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = useCallback(async (mode: Mode, text: string, range: TimeRange) => {
    setLoading(true);
    setError(null);
    try {
      setResult(await runSearch(mode, text, range));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setResult(null);
    } finally {
      setLoading(false);
    }
  }, []);

  return { result, loading, error, run };
}
