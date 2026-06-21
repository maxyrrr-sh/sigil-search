import { useState } from 'react';
import {
  EuiButton,
  EuiButtonEmpty,
  EuiButtonGroup,
  EuiFieldText,
  EuiForm,
  EuiFormRow,
  EuiModal,
  EuiModalBody,
  EuiModalFooter,
  EuiModalHeader,
  EuiModalHeaderTitle,
  EuiSelect,
} from '@elastic/eui';

import type { Mode } from '../api/types';
import type { Dashboard, PanelDef, VizType } from '../state/useDashboards';

interface Props {
  onClose: () => void;
  onSave: (dashboardId: string, panel: Omit<PanelDef, 'id'>) => void;
  dashboards: Dashboard[];
  createDashboard: (name: string) => string;
  initial?: Partial<Pick<PanelDef, 'title' | 'mode' | 'query' | 'viz'>>;
  pickDashboard: boolean; // true: choose/create a target (from Search); false: current dashboard
  dashboardId?: string;
}

const VIZ: { value: VizType; text: string }[] = [
  { value: 'metric', text: 'Metric (single number)' },
  { value: 'bar', text: 'Bar chart (aggregation)' },
  { value: 'table', text: 'Table' },
  { value: 'timeseries', text: 'Time series (events)' },
];

export function PanelFormModal({ onClose, onSave, dashboards, createDashboard, initial, pickDashboard, dashboardId }: Props) {
  const [title, setTitle] = useState(initial?.title ?? '');
  const [mode, setMode] = useState<Mode>(initial?.mode ?? 'dsl');
  const [query, setQuery] = useState(initial?.query ?? '');
  const [viz, setViz] = useState<VizType>(initial?.viz ?? 'metric');
  const [target, setTarget] = useState<string>(dashboards[0]?.id ?? 'new');
  const [newName, setNewName] = useState('');

  const save = () => {
    let dashId = dashboardId ?? '';
    if (pickDashboard) dashId = target === 'new' ? createDashboard(newName || 'New dashboard') : target;
    onSave(dashId, { title: title || query || 'Panel', mode, query, viz });
    onClose();
  };

  return (
    <EuiModal onClose={onClose} style={{ width: 560 }}>
      <EuiModalHeader>
        <EuiModalHeaderTitle>{pickDashboard ? 'Add to dashboard' : 'Add panel'}</EuiModalHeaderTitle>
      </EuiModalHeader>
      <EuiModalBody>
        <EuiForm component="form">
          {pickDashboard && (
            <>
              <EuiFormRow label="Dashboard">
                <EuiSelect
                  value={target}
                  onChange={(e) => setTarget(e.target.value)}
                  options={[{ value: 'new', text: '➕ New dashboard…' }, ...dashboards.map((d) => ({ value: d.id, text: d.name }))]}
                />
              </EuiFormRow>
              {target === 'new' && (
                <EuiFormRow label="New dashboard name">
                  <EuiFieldText value={newName} onChange={(e) => setNewName(e.target.value)} placeholder="My dashboard" />
                </EuiFormRow>
              )}
            </>
          )}
          <EuiFormRow label="Panel title">
            <EuiFieldText value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Errors by host" />
          </EuiFormRow>
          <EuiFormRow label="Query language">
            <EuiButtonGroup
              legend="Query mode"
              type="single"
              buttonSize="compressed"
              idSelected={mode}
              onChange={(id) => setMode(id as Mode)}
              options={[
                { id: 'dsl', label: 'Pipe-DSL' },
                { id: 'lucene', label: 'Lucene' },
                { id: 'sql', label: 'SQL' },
              ]}
            />
          </EuiFormRow>
          <EuiFormRow label="Query">
            <EuiFieldText value={query} onChange={(e) => setQuery(e.target.value)} placeholder="stats count by host" fullWidth />
          </EuiFormRow>
          <EuiFormRow label="Visualization">
            <EuiSelect value={viz} onChange={(e) => setViz(e.target.value as VizType)} options={VIZ} />
          </EuiFormRow>
        </EuiForm>
      </EuiModalBody>
      <EuiModalFooter>
        <EuiButtonEmpty onClick={onClose}>Cancel</EuiButtonEmpty>
        <EuiButton fill onClick={save}>
          {pickDashboard ? 'Add panel' : 'Add'}
        </EuiButton>
      </EuiModalFooter>
    </EuiModal>
  );
}
