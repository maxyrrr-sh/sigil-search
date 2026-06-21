import {
  EuiAccordion,
  EuiBadge,
  EuiFlexGroup,
  EuiFlexItem,
  EuiPanel,
  EuiText,
  EuiTitle,
  EuiSpacer,
  useEuiTheme,
} from '@elastic/eui';

import type { EventRow } from '../api/types';

interface Props {
  events: EventRow[];
  onAddFilter: (field: string, value: string) => void;
}

export function FieldSidebar({ events, onAddFilter }: Props) {
  const { euiTheme } = useEuiTheme();
  const fieldCounts: Record<string, number> = {};
  const valueCounts: Record<string, Record<string, number>> = {};

  for (const e of events) {
    for (const [k, v] of Object.entries(e.fields)) {
      fieldCounts[k] = (fieldCounts[k] || 0) + 1;
      (valueCounts[k] ||= {})[v] = (valueCounts[k][v] || 0) + 1;
    }
  }

  const fields = Object.keys(fieldCounts)
    .sort((a, b) => fieldCounts[b] - fieldCounts[a])
    .slice(0, 30);

  return (
    <EuiPanel paddingSize="s" hasShadow={false} hasBorder>
      <EuiTitle size="xxs">
        <h3>Fields ({fields.length})</h3>
      </EuiTitle>
      <EuiSpacer size="s" />
      {fields.length === 0 && (
        <EuiText size="xs" color="subdued">
          No fields in the current results.
        </EuiText>
      )}
      {fields.map((field) => {
        const top = Object.entries(valueCounts[field])
          .sort(([, a], [, b]) => b - a)
          .slice(0, 5);
        return (
          <EuiAccordion
            id={`field-${field}`}
            key={field}
            paddingSize="s"
            buttonContent={
              <EuiFlexGroup gutterSize="xs" alignItems="center" responsive={false}>
                <EuiFlexItem>
                  <EuiText size="xs" style={{ wordBreak: 'break-all' }}>
                    {field}
                  </EuiText>
                </EuiFlexItem>
                <EuiFlexItem grow={false}>
                  <EuiBadge color="hollow">{fieldCounts[field]}</EuiBadge>
                </EuiFlexItem>
              </EuiFlexGroup>
            }
          >
            {top.map(([value, count]) => (
              <EuiFlexGroup key={value} gutterSize="xs" alignItems="center" responsive={false} style={{ marginTop: 2 }}>
                <EuiFlexItem>
                  <button
                    type="button"
                    onClick={() => onAddFilter(field, value)}
                    title={`Filter on ${field} = ${value}`}
                    style={{
                      background: 'none',
                      border: 'none',
                      padding: 0,
                      textAlign: 'left',
                      cursor: 'pointer',
                      color: euiTheme.colors.primaryText,
                      fontSize: 12,
                      wordBreak: 'break-all',
                    }}
                  >
                    {value || '(empty)'}
                  </button>
                </EuiFlexItem>
                <EuiFlexItem grow={false}>
                  <EuiText size="xs" color="subdued">
                    {count}
                  </EuiText>
                </EuiFlexItem>
              </EuiFlexGroup>
            ))}
          </EuiAccordion>
        );
      })}
    </EuiPanel>
  );
}
