import { EuiFlexGroup, EuiFlexItem, EuiText, useEuiTheme } from '@elastic/eui';

export interface Bar {
  label: string;
  value: number;
}

// A dependency-free horizontal bar chart themed with EUI colors.
export function BarChartH({ items, max: maxProp, limit = 50 }: { items: Bar[]; max?: number; limit?: number }) {
  const { euiTheme } = useEuiTheme();
  const max = maxProp ?? Math.max(1, ...items.map((b) => b.value));
  return (
    <div>
      {items.slice(0, limit).map((b, i) => (
        <div key={i} style={{ marginBottom: 6 }}>
          <EuiFlexGroup gutterSize="s" alignItems="center" responsive={false}>
            <EuiFlexItem grow={false} style={{ width: 150 }}>
              <EuiText size="xs" className="eui-textTruncate" title={b.label}>
                {b.label || '(empty)'}
              </EuiText>
            </EuiFlexItem>
            <EuiFlexItem>
              <div style={{ background: euiTheme.colors.lightestShade, borderRadius: 3 }}>
                <div
                  style={{
                    width: `${(b.value / max) * 100}%`,
                    minWidth: 2,
                    height: 14,
                    background: euiTheme.colors.primary,
                    borderRadius: 3,
                  }}
                />
              </div>
            </EuiFlexItem>
            <EuiFlexItem grow={false} style={{ width: 56, textAlign: 'right' }}>
              <EuiText size="xs">{b.value}</EuiText>
            </EuiFlexItem>
          </EuiFlexGroup>
        </div>
      ))}
    </div>
  );
}

// Detect a category + numeric column pair from aggregation rows.
export function barsFromRows(rows: Record<string, unknown>[], columns: string[]): Bar[] {
  const isNum = (v: unknown) => typeof v === 'number' || (typeof v === 'string' && v.trim() !== '' && !Number.isNaN(Number(v)));
  const numericCol =
    columns.find((c) => c === 'count' || c === 'c') ||
    columns.find((c) => rows.length > 0 && rows.every((r) => isNum(r[c])));
  const categoryCol = columns.find((c) => c !== numericCol);
  if (!numericCol || !categoryCol) return [];
  return rows
    .map((r) => ({ label: String(r[categoryCol] ?? ''), value: Number(r[numericCol]) }))
    .filter((b) => Number.isFinite(b.value));
}
