import {
  EuiButton,
  EuiButtonGroup,
  EuiFieldText,
  EuiFlexGroup,
  EuiFlexItem,
  EuiSuperDatePicker,
} from '@elastic/eui';

import type { Mode, TimeRange } from '../api/types';

const PLACEHOLDER: Record<Mode, string> = {
  dsl: 'search error | stats count by host',
  lucene: 'message:Failed OR host:web1',
  sql: 'SELECT log_level, count(*) AS c FROM events GROUP BY log_level',
};

interface Props {
  mode: Mode;
  onModeChange: (m: Mode) => void;
  text: string;
  onTextChange: (t: string) => void;
  range: TimeRange;
  onRangeChange: (r: TimeRange) => void;
  onSearch: () => void;
  loading: boolean;
}

export function SearchBar({ mode, onModeChange, text, onTextChange, range, onRangeChange, onSearch, loading }: Props) {
  return (
    <EuiFlexGroup gutterSize="s" alignItems="center" responsive={false}>
      <EuiFlexItem grow={false}>
        <EuiButtonGroup
          legend="Query mode"
          buttonSize="compressed"
          type="single"
          idSelected={mode}
          onChange={(id) => onModeChange(id as Mode)}
          options={[
            { id: 'dsl', label: 'Pipe-DSL' },
            { id: 'lucene', label: 'Lucene' },
            { id: 'sql', label: 'SQL' },
          ]}
        />
      </EuiFlexItem>
      <EuiFlexItem>
        <EuiFieldText
          fullWidth
          value={text}
          onChange={(e) => onTextChange(e.target.value)}
          placeholder={PLACEHOLDER[mode]}
          aria-label="Search query"
          onKeyDown={(e) => {
            if (e.key === 'Enter') onSearch();
          }}
        />
      </EuiFlexItem>
      <EuiFlexItem grow={false}>
        <EuiSuperDatePicker
          start={range.from}
          end={range.to}
          onTimeChange={({ start, end }) => onRangeChange({ from: start, to: end })}
          onRefresh={() => onSearch()}
          showUpdateButton={false}
          width="auto"
        />
      </EuiFlexItem>
      <EuiFlexItem grow={false}>
        <EuiButton fill iconType="search" onClick={onSearch} isLoading={loading}>
          Search
        </EuiButton>
      </EuiFlexItem>
    </EuiFlexGroup>
  );
}
