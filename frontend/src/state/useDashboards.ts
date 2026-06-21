import { useCallback, useState } from 'react';

import type { Mode } from '../api/types';

export type VizType = 'metric' | 'bar' | 'table' | 'timeseries';

export interface PanelDef {
  id: string;
  title: string;
  mode: Mode;
  query: string;
  viz: VizType;
}

export interface Dashboard {
  id: string;
  name: string;
  panels: PanelDef[];
  createdAt: number;
}

const KEY = 'sigil.dashboards';

function load(): Dashboard[] {
  try {
    return JSON.parse(localStorage.getItem(KEY) || '[]');
  } catch {
    return [];
  }
}

const uid = () => Math.random().toString(36).slice(2, 10);

export function useDashboards() {
  const [dashboards, setDashboards] = useState<Dashboard[]>(load);

  const persist = useCallback((next: Dashboard[]) => {
    localStorage.setItem(KEY, JSON.stringify(next));
    setDashboards(next);
    return next;
  }, []);

  const createDashboard = useCallback(
    (name: string): string => {
      const id = uid();
      persist([...load(), { id, name: name || 'Untitled', panels: [], createdAt: Date.now() }]);
      return id;
    },
    [persist],
  );

  const deleteDashboard = useCallback(
    (id: string) => persist(load().filter((d) => d.id !== id)),
    [persist],
  );

  const renameDashboard = useCallback(
    (id: string, name: string) => persist(load().map((d) => (d.id === id ? { ...d, name } : d))),
    [persist],
  );

  const addPanel = useCallback(
    (dashId: string, panel: Omit<PanelDef, 'id'>) =>
      persist(load().map((d) => (d.id === dashId ? { ...d, panels: [...d.panels, { ...panel, id: uid() }] } : d))),
    [persist],
  );

  const removePanel = useCallback(
    (dashId: string, panelId: string) =>
      persist(load().map((d) => (d.id === dashId ? { ...d, panels: d.panels.filter((p) => p.id !== panelId) } : d))),
    [persist],
  );

  return { dashboards, createDashboard, deleteDashboard, renameDashboard, addPanel, removePanel };
}
