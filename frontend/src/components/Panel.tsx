import { useEffect, useState } from 'react';
import {
  EuiBadge,
  EuiBasicTable,
  EuiButtonIcon,
  EuiCallOut,
  EuiFlexGroup,
  EuiFlexItem,
  EuiLoadingSpinner,
  EuiPanel,
  EuiSpacer,
  EuiStat,
  EuiText,
  EuiTitle,
} from '@elastic/eui';

import type { QueryResult, TimeRange } from '../api/types';
import { fmtTime, summaryOf } from '../lib/format';
import { buildHistogram, runSearch } from '../lib/query';
import type { PanelDef } from '../state/useDashboards';
import { BarChartH, barsFromRows } from './BarChartH';
import { Histogram } from './Histogram';

const RANGE: TimeRange = { from: 'now-24h', to: 'now' };

function metricValue(result: QueryResult): { value: string; label: string } {
  if (result.kind === 'events') return { value: String(result.total), label: 'events' };
  const numericCol =
    result.columns.find((c) => c === 'count' || c === 'c') ||
    result.columns.find((c) => result.rows.every((r) => !Number.isNaN(Number(r[c]))));
  if (numericCol && result.rows.length) {
    const sum = result.rows.reduce((a, r) => a + Number(r[numericCol] ?? 0), 0);
    return { value: String(sum), label: numericCol };
  }
  return { value: String(result.total), label: 'rows' };
}

function PanelBody({ def, result }: { def: PanelDef; result: QueryResult }) {
  switch (def.viz) {
    case 'metric': {
      const m = metricValue(result);
      return <EuiStat title={m.value} description={m.label} titleColor="primary" />;
    }
    case 'bar': {
      const bars = barsFromRows(result.rows, result.columns);
      if (!bars.length)
        return <EuiText size="s" color="subdued">Use a <code>stats</code> / GROUP BY query for a bar chart.</EuiText>;
      return <BarChartH items={bars} limit={12} />;
    }
    case 'timeseries': {
      if (!result.events.length)
        return <EuiText size="s" color="subdued">No events to plot (this looks like an aggregation).</EuiText>;
      return <Histogram buckets={buildHistogram(result.events, RANGE)} />;
    }
    case 'table':
    default: {
      if (result.kind === 'stats') {
        const columns = result.columns.map((c) => ({ field: c, name: c, truncateText: true }));
        return <EuiBasicTable items={result.rows} columns={columns} responsiveBreakpoint={false} />;
      }
      const items = result.events.slice(0, 50).map((e, i) => ({ _id: String(i), ts: e.ts, msg: summaryOf(e.fields) }));
      return (
        <EuiBasicTable
          items={items}
          itemId="_id"
          responsiveBreakpoint={false}
          columns={[
            { name: 'Time', width: '200px', render: (it: { ts: number | null }) => <code>{fmtTime(it.ts)}</code> },
            { name: 'Event', field: 'msg', truncateText: true },
          ]}
        />
      );
    }
  }
}

export function Panel({ def, onRemove }: { def: PanelDef; onRemove: () => void }) {
  const [result, setResult] = useState<QueryResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = () => {
    setLoading(true);
    runSearch(def.mode, def.query, RANGE)
      .then((r) => {
        setResult(r);
        setError(null);
      })
      .catch((e) => setError(e instanceof Error ? e.message : String(e)))
      .finally(() => setLoading(false));
  };

  useEffect(load, [def.id, def.mode, def.query]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <EuiPanel paddingSize="m" hasShadow={false} hasBorder style={{ height: '100%' }}>
      <EuiFlexGroup alignItems="center" gutterSize="s" responsive={false}>
        <EuiFlexItem>
          <EuiTitle size="xxs">
            <h3>{def.title}</h3>
          </EuiTitle>
        </EuiFlexItem>
        <EuiFlexItem grow={false}>
          <EuiBadge color="hollow">{def.mode}</EuiBadge>
        </EuiFlexItem>
        <EuiFlexItem grow={false}>
          <EuiButtonIcon iconType="refresh" aria-label="Refresh panel" onClick={load} />
        </EuiFlexItem>
        <EuiFlexItem grow={false}>
          <EuiButtonIcon iconType="trash" color="danger" aria-label="Remove panel" onClick={onRemove} />
        </EuiFlexItem>
      </EuiFlexGroup>
      <EuiText size="xs" color="subdued" className="eui-textTruncate" title={def.query}>
        <code>{def.query || '(all events)'}</code>
      </EuiText>
      <EuiSpacer size="s" />
      {loading && <EuiLoadingSpinner size="l" />}
      {error && (
        <EuiCallOut color="danger" size="s" title="Panel query failed">
          <EuiText size="xs">{error}</EuiText>
        </EuiCallOut>
      )}
      {!loading && !error && result && <PanelBody def={def} result={result} />}
    </EuiPanel>
  );
}
