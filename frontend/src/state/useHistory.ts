import { useCallback, useState } from 'react';

import type { Mode } from '../api/types';

export interface HistoryEntry {
  mode: Mode;
  text: string;
  at: number;
}

export interface SavedSearch {
  name: string;
  mode: Mode;
  text: string;
}

const RECENT_KEY = 'sigil.recent';
const SAVED_KEY = 'sigil.saved';

function load<T>(key: string): T[] {
  try {
    return JSON.parse(localStorage.getItem(key) || '[]');
  } catch {
    return [];
  }
}

export function useHistory() {
  const [recent, setRecent] = useState<HistoryEntry[]>(() => load<HistoryEntry>(RECENT_KEY));
  const [saved, setSaved] = useState<SavedSearch[]>(() => load<SavedSearch>(SAVED_KEY));

  const remember = useCallback((mode: Mode, text: string) => {
    if (!text.trim()) return;
    setRecent((prev) => {
      const next = [{ mode, text, at: Date.now() }, ...prev.filter((r) => !(r.mode === mode && r.text === text))].slice(0, 20);
      localStorage.setItem(RECENT_KEY, JSON.stringify(next));
      return next;
    });
  }, []);

  const save = useCallback((name: string, mode: Mode, text: string) => {
    setSaved((prev) => {
      const next = [{ name, mode, text }, ...prev.filter((s) => s.name !== name)].slice(0, 50);
      localStorage.setItem(SAVED_KEY, JSON.stringify(next));
      return next;
    });
  }, []);

  const removeSaved = useCallback((name: string) => {
    setSaved((prev) => {
      const next = prev.filter((s) => s.name !== name);
      localStorage.setItem(SAVED_KEY, JSON.stringify(next));
      return next;
    });
  }, []);

  return { recent, saved, remember, save, removeSaved };
}
