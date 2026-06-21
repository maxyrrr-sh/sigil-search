import { useState } from 'react';
import {
  EuiButton,
  EuiButtonEmpty,
  EuiButtonIcon,
  EuiEmptyPrompt,
  EuiFieldText,
  EuiFlexGroup,
  EuiFlexItem,
  EuiModal,
  EuiModalBody,
  EuiModalFooter,
  EuiModalHeader,
  EuiModalHeaderTitle,
  EuiPanel,
  EuiSpacer,
  EuiText,
  EuiTitle,
} from '@elastic/eui';

import { useDashboards } from '../state/useDashboards';
import { Panel } from './Panel';
import { PanelFormModal } from './PanelFormModal';

function CreateModal({ onClose, onCreate }: { onClose: () => void; onCreate: (name: string) => void }) {
  const [name, setName] = useState('');
  return (
    <EuiModal onClose={onClose} style={{ width: 420 }}>
      <EuiModalHeader>
        <EuiModalHeaderTitle>New dashboard</EuiModalHeaderTitle>
      </EuiModalHeader>
      <EuiModalBody>
        <EuiFieldText
          autoFocus
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Security overview"
          onKeyDown={(e) => {
            if (e.key === 'Enter' && name.trim()) {
              onCreate(name);
              onClose();
            }
          }}
        />
      </EuiModalBody>
      <EuiModalFooter>
        <EuiButtonEmpty onClick={onClose}>Cancel</EuiButtonEmpty>
        <EuiButton fill onClick={() => { onCreate(name || 'Untitled'); onClose(); }}>
          Create
        </EuiButton>
      </EuiModalFooter>
    </EuiModal>
  );
}

export function DashboardsView() {
  const { dashboards, createDashboard, deleteDashboard, addPanel, removePanel } = useDashboards();
  const [selectedId, setSelectedId] = useState<string | null>(dashboards[0]?.id ?? null);
  const [showCreate, setShowCreate] = useState(false);
  const [showAddPanel, setShowAddPanel] = useState(false);

  const selected = dashboards.find((d) => d.id === selectedId) ?? dashboards[0] ?? null;

  if (dashboards.length === 0) {
    return (
      <>
        <EuiEmptyPrompt
          iconType="dashboardApp"
          title={<h2>No dashboards yet</h2>}
          body={<p>Create a dashboard, then add panels — or use “Add to dashboard” from a search.</p>}
          actions={<EuiButton fill iconType="plusInCircle" onClick={() => setShowCreate(true)}>Create dashboard</EuiButton>}
        />
        {showCreate && (
          <CreateModal
            onClose={() => setShowCreate(false)}
            onCreate={(name) => setSelectedId(createDashboard(name))}
          />
        )}
      </>
    );
  }

  return (
    <EuiFlexGroup>
      {/* dashboard list */}
      <EuiFlexItem grow={false} style={{ width: 220 }}>
        <EuiPanel paddingSize="s" hasShadow={false} hasBorder>
          <EuiFlexGroup alignItems="center" gutterSize="s" responsive={false}>
            <EuiFlexItem>
              <EuiTitle size="xxs"><h3>Dashboards</h3></EuiTitle>
            </EuiFlexItem>
            <EuiFlexItem grow={false}>
              <EuiButtonIcon iconType="plusInCircle" aria-label="New dashboard" onClick={() => setShowCreate(true)} />
            </EuiFlexItem>
          </EuiFlexGroup>
          <EuiSpacer size="s" />
          {dashboards.map((d) => (
            <EuiButtonEmpty
              key={d.id}
              size="s"
              flush="left"
              color={d.id === selected?.id ? 'primary' : 'text'}
              onClick={() => setSelectedId(d.id)}
              style={{ display: 'block', textAlign: 'left' }}
            >
              {d.name} ({d.panels.length})
            </EuiButtonEmpty>
          ))}
        </EuiPanel>
      </EuiFlexItem>

      {/* selected dashboard */}
      <EuiFlexItem>
        {selected && (
          <>
            <EuiFlexGroup alignItems="center" gutterSize="s" responsive={false}>
              <EuiFlexItem>
                <EuiTitle size="s"><h2>{selected.name}</h2></EuiTitle>
              </EuiFlexItem>
              <EuiFlexItem grow={false}>
                <EuiButton size="s" iconType="plusInCircle" onClick={() => setShowAddPanel(true)}>Add panel</EuiButton>
              </EuiFlexItem>
              <EuiFlexItem grow={false}>
                <EuiButtonIcon iconType="trash" color="danger" aria-label="Delete dashboard"
                  onClick={() => { deleteDashboard(selected.id); setSelectedId(null); }} />
              </EuiFlexItem>
            </EuiFlexGroup>
            <EuiSpacer size="m" />
            {selected.panels.length === 0 ? (
              <EuiText color="subdued">No panels yet — click <strong>Add panel</strong>.</EuiText>
            ) : (
              <EuiFlexGroup wrap>
                {selected.panels.map((p) => (
                  <EuiFlexItem key={p.id} style={{ minWidth: 360, flexBasis: 'calc(50% - 8px)' }}>
                    <Panel def={p} onRemove={() => removePanel(selected.id, p.id)} />
                  </EuiFlexItem>
                ))}
              </EuiFlexGroup>
            )}
          </>
        )}
      </EuiFlexItem>

      {showCreate && (
        <CreateModal onClose={() => setShowCreate(false)} onCreate={(name) => setSelectedId(createDashboard(name))} />
      )}
      {showAddPanel && selected && (
        <PanelFormModal
          onClose={() => setShowAddPanel(false)}
          onSave={(_dashId, panel) => addPanel(selected.id, panel)}
          dashboards={dashboards}
          createDashboard={createDashboard}
          pickDashboard={false}
          dashboardId={selected.id}
        />
      )}
    </EuiFlexGroup>
  );
}
