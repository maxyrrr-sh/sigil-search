import { useCallback, useEffect, useState } from 'react';
import {
  EuiBadge,
  EuiButton,
  EuiCallOut,
  EuiCode,
  EuiFlexGroup,
  EuiFlexItem,
  EuiLoadingSpinner,
  EuiPanel,
  EuiSpacer,
  EuiTabbedContent,
  EuiText,
  type EuiTabbedContentTab,
} from '@elastic/eui';

import type { Mode, TimeRange } from '../api/types';
import { buildHistogram } from '../lib/query';
import { useDashboards } from '../state/useDashboards';
import { useHistory } from '../state/useHistory';
import { useSearch } from '../state/useSearch';
import { EventsTable } from './EventsTable';
import { FieldSidebar } from './FieldSidebar';
import { Histogram } from './Histogram';
import { PanelFormModal } from './PanelFormModal';
import { SearchBar } from './SearchBar';
import { StatsView } from './StatsView';

export function SearchView() {
  const [mode, setMode] = useState<Mode>('dsl');
  const [text, setText] = useState('');
  const [range, setRange] = useState<TimeRange>({ from: 'now-24h', to: 'now' });
  const [tab, setTab] = useState('events');
  const [showAdd, setShowAdd] = useState(false);

  const { result, loading, error, run } = useSearch();
  const { recent, remember } = useHistory();
  const { dashboards, createDashboard, addPanel } = useDashboards();

  const doSearch = useCallback(
    (m: Mode = mode, t: string = text, r: TimeRange = range) => {
      remember(m, t);
      run(m, t, r);
    },
    [mode, text, range, remember, run],
  );

  useEffect(() => {
    run('dsl', '', { from: 'now-24h', to: 'now' });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (result) setTab(result.kind === 'stats' ? 'stats' : 'events');
  }, [result]);

  const onRangeChange = (r: TimeRange) => {
    setRange(r);
    run(mode, text, r);
  };

  const onAddFilter = (field: string, value: string) => {
    let t = text;
    if (mode === 'lucene') t = (text ? `${text} AND ` : '') + `${field}:"${value}"`;
    else if (mode === 'dsl') t = `${text || 'search'} | where ${field} == "${value}"`;
    else return;
    setText(t);
    remember(mode, t);
    run(mode, t, range);
  };

  const events = result?.events ?? [];

  const eventsTab = (
    <>
      <EuiSpacer size="m" />
      <EuiFlexGroup>
        <EuiFlexItem grow={false} style={{ width: 250 }}>
          <FieldSidebar events={events} onAddFilter={onAddFilter} />
        </EuiFlexItem>
        <EuiFlexItem>
          <EuiPanel paddingSize="m" hasShadow={false} hasBorder>
            <Histogram buckets={buildHistogram(events, range)} />
          </EuiPanel>
          <EuiSpacer size="m" />
          <EuiPanel paddingSize="none" hasShadow={false} hasBorder>
            <EventsTable events={events} />
          </EuiPanel>
        </EuiFlexItem>
      </EuiFlexGroup>
    </>
  );

  const statsTab = (
    <>
      <EuiSpacer size="m" />
      <StatsView rows={result?.rows ?? []} columns={result?.columns ?? []} />
    </>
  );

  const tabs: EuiTabbedContentTab[] = [
    { id: 'events', name: `Events${result?.kind === 'events' ? ` (${result.total})` : ''}`, content: eventsTab },
    { id: 'stats', name: `Statistics${result?.kind === 'stats' ? ` (${result.total})` : ''}`, content: statsTab },
  ];

  return (
    <>
      <SearchBar
        mode={mode}
        onModeChange={setMode}
        text={text}
        onTextChange={setText}
        range={range}
        onRangeChange={onRangeChange}
        onSearch={() => doSearch()}
        loading={loading}
      />

      {recent.length > 0 && (
        <>
          <EuiSpacer size="s" />
          <EuiFlexGroup gutterSize="xs" alignItems="center" responsive={false} wrap>
            <EuiFlexItem grow={false}>
              <EuiText size="xs" color="subdued">Recent:</EuiText>
            </EuiFlexItem>
            {recent.slice(0, 6).map((r, i) => (
              <EuiFlexItem grow={false} key={i}>
                <EuiBadge
                  color="hollow"
                  iconType="clock"
                  onClick={() => { setMode(r.mode); setText(r.text); doSearch(r.mode, r.text, range); }}
                  onClickAriaLabel="Run recent query"
                >
                  {r.mode}: {r.text || '(all)'}
                </EuiBadge>
              </EuiFlexItem>
            ))}
          </EuiFlexGroup>
        </>
      )}

      <EuiSpacer size="m" />

      {error && (
        <EuiCallOut color="danger" iconType="alert" title="Query failed">
          <EuiText size="s">{error}</EuiText>
        </EuiCallOut>
      )}

      {!error && result && (
        <>
          <EuiFlexGroup alignItems="center" gutterSize="s" responsive={false} wrap>
            <EuiFlexItem grow={false}>
              <EuiText size="s">
                <strong>{result.total}</strong> {result.kind === 'events' ? 'events' : 'rows'}
              </EuiText>
            </EuiFlexItem>
            {result.sql && (
              <EuiFlexItem grow={false}>
                <EuiText size="xs" color="subdued">SQL: <EuiCode>{result.sql}</EuiCode></EuiText>
              </EuiFlexItem>
            )}
            <EuiFlexItem grow={false}>
              <EuiButton size="s" iconType="dashboardApp" onClick={() => setShowAdd(true)}>Add to dashboard</EuiButton>
            </EuiFlexItem>
          </EuiFlexGroup>
          <EuiSpacer size="s" />
          <EuiTabbedContent tabs={tabs} selectedTab={tabs.find((t) => t.id === tab)} onTabClick={(t) => setTab(t.id)} />
        </>
      )}

      {!error && !result && loading && (
        <div style={{ textAlign: 'center', padding: 40 }}>
          <EuiLoadingSpinner size="xl" />
        </div>
      )}

      {showAdd && (
        <PanelFormModal
          onClose={() => setShowAdd(false)}
          onSave={(dashId, panel) => addPanel(dashId, panel)}
          dashboards={dashboards}
          createDashboard={createDashboard}
          pickDashboard
          initial={{ title: text || 'Events', mode, query: text, viz: result?.kind === 'stats' ? 'bar' : 'timeseries' }}
        />
      )}
    </>
  );
}
