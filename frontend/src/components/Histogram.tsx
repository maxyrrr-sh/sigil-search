import { useEuiTheme, EuiText, EuiFlexGroup, EuiFlexItem } from '@elastic/eui';

import type { HistogramBucket } from '../api/types';
import { fmtClock } from '../lib/format';

// A lightweight, dependency-free SVG bar chart of events over time.
export function Histogram({ buckets }: { buckets: HistogramBucket[] }) {
  const { euiTheme } = useEuiTheme();
  const max = Math.max(1, ...buckets.map((b) => b.count));
  const total = buckets.reduce((a, b) => a + b.count, 0);
  const W = 1000;
  const H = 110;
  const pad = 8;
  const n = buckets.length || 1;
  const bw = (W - pad * 2) / n;
  const first = buckets[0];
  const mid = buckets[Math.floor(n / 2)];
  const last = buckets[n - 1];

  return (
    <div>
      <svg
        viewBox={`0 0 ${W} ${H}`}
        width="100%"
        height={H}
        preserveAspectRatio="none"
        role="img"
        aria-label="Events over time"
      >
        {buckets.map((b, i) => {
          const h = (b.count / max) * (H - pad * 2);
          return (
            <rect
              key={i}
              x={pad + i * bw + 0.5}
              y={H - pad - h}
              width={Math.max(1, bw - 1)}
              height={h}
              fill={euiTheme.colors.primary}
            >
              <title>{`${fmtClock(b.t)} — ${b.count} event${b.count === 1 ? '' : 's'}`}</title>
            </rect>
          );
        })}
        <line x1={pad} y1={H - pad} x2={W - pad} y2={H - pad} stroke={euiTheme.colors.lightShade} strokeWidth={1} />
      </svg>
      <EuiFlexGroup justifyContent="spaceBetween" gutterSize="none" style={{ marginTop: 2 }}>
        <EuiFlexItem grow={false}>
          <EuiText size="xs" color="subdued">{first ? fmtClock(first.t) : ''}</EuiText>
        </EuiFlexItem>
        <EuiFlexItem grow={false}>
          <EuiText size="xs" color="subdued">{`${total} events`}</EuiText>
        </EuiFlexItem>
        <EuiFlexItem grow={false}>
          <EuiText size="xs" color="subdued">{last ? fmtClock(last.t) : ''}</EuiText>
        </EuiFlexItem>
      </EuiFlexGroup>
    </div>
  );
}
