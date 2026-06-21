import {
  EuiBasicTable,
  EuiFlexGroup,
  EuiFlexItem,
  EuiPanel,
  EuiSpacer,
  EuiText,
  EuiTitle,
  type EuiBasicTableColumn,
} from '@elastic/eui';

import { BarChartH, barsFromRows } from './BarChartH';

type Row = Record<string, unknown>;

export function StatsView({ rows, columns }: { rows: Row[]; columns: string[] }) {
  const tableColumns: EuiBasicTableColumn<Row>[] = columns.map((c) => ({
    field: c,
    name: c,
    truncateText: true,
    render: (value: unknown) => <EuiText size="s">{value == null ? '' : String(value)}</EuiText>,
  }));

  const bars = barsFromRows(rows, columns);

  return (
    <EuiFlexGroup>
      <EuiFlexItem grow={2}>
        <EuiPanel paddingSize="s" hasShadow={false} hasBorder>
          <EuiTitle size="xxs">
            <h3>Statistics ({rows.length} rows)</h3>
          </EuiTitle>
          <EuiSpacer size="s" />
          <EuiBasicTable items={rows} columns={tableColumns} responsiveBreakpoint={false} />
        </EuiPanel>
      </EuiFlexItem>
      {bars.length > 0 && (
        <EuiFlexItem grow={3}>
          <EuiPanel paddingSize="m" hasShadow={false} hasBorder>
            <EuiTitle size="xxs">
              <h3>Distribution</h3>
            </EuiTitle>
            <EuiSpacer size="m" />
            <BarChartH items={bars} />
          </EuiPanel>
        </EuiFlexItem>
      )}
    </EuiFlexGroup>
  );
}
