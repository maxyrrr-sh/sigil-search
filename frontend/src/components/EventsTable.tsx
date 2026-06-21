import { useState, type ReactNode } from 'react';
import {
  EuiBasicTable,
  EuiBadge,
  EuiButtonIcon,
  EuiCodeBlock,
  EuiDescriptionList,
  EuiFlexGroup,
  EuiFlexItem,
  EuiPanel,
  EuiText,
  type EuiBasicTableColumn,
} from '@elastic/eui';

import type { EventRow } from '../api/types';
import { fmtTime, summaryOf } from '../lib/format';

type Item = EventRow & { _id: string };

function ExpandedRow({ row }: { row: EventRow }) {
  const listItems = Object.entries(row.fields)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([title, description]) => ({ title, description }));
  return (
    <EuiPanel color="subdued" paddingSize="m" hasShadow={false} style={{ width: '100%' }}>
      <EuiFlexGroup>
        <EuiFlexItem>
          <EuiDescriptionList type="column" compressed listItems={listItems} />
        </EuiFlexItem>
        <EuiFlexItem>
          <EuiCodeBlock language="json" fontSize="s" paddingSize="s" isCopyable overflowHeight={320}>
            {JSON.stringify(row.raw, null, 2)}
          </EuiCodeBlock>
        </EuiFlexItem>
      </EuiFlexGroup>
    </EuiPanel>
  );
}

export function EventsTable({ events }: { events: EventRow[] }) {
  const [expanded, setExpanded] = useState<Record<string, ReactNode>>({});
  const items: Item[] = events.map((e, i) => ({ ...e, _id: String(i) }));

  const toggle = (item: Item) => {
    setExpanded((prev) => {
      const next = { ...prev };
      if (next[item._id]) delete next[item._id];
      else next[item._id] = <ExpandedRow row={item} />;
      return next;
    });
  };

  const columns: EuiBasicTableColumn<Item>[] = [
    {
      align: 'left',
      width: '40px',
      isExpander: true,
      name: '',
      render: (item: Item) => (
        <EuiButtonIcon
          onClick={() => toggle(item)}
          aria-label={expanded[item._id] ? 'Collapse' : 'Expand'}
          iconType={expanded[item._id] ? 'arrowDown' : 'arrowRight'}
        />
      ),
    },
    {
      name: 'Time',
      width: '215px',
      render: (item: Item) => (
        <EuiText size="s">
          <code>{fmtTime(item.ts)}</code>
        </EuiText>
      ),
    },
    {
      name: 'dataset',
      width: '140px',
      render: (item: Item) => (item.dataset ? <EuiBadge color="hollow">{item.dataset}</EuiBadge> : null),
    },
    {
      name: 'Event',
      render: (item: Item) => <EuiText size="s">{summaryOf(item.fields)}</EuiText>,
    },
  ];

  return (
    <EuiBasicTable
      items={items}
      itemId="_id"
      columns={columns}
      itemIdToExpandedRowMap={expanded}
      responsiveBreakpoint={false}
      noItemsMessage="No events match this search and time range."
    />
  );
}
