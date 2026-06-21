// Small formatting helpers. Sigil timestamps are epoch MICROSECONDS.

export function microsToDate(micros: number): Date {
  return new Date(Math.floor(micros / 1000));
}

export function fmtTime(micros: number | null | undefined): string {
  if (micros == null || Number.isNaN(micros)) return '—';
  return microsToDate(micros).toISOString().replace('T', ' ').replace('Z', '');
}

export function fmtClock(ms: number): string {
  const d = new Date(ms);
  return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

export function fieldsToRecord(fields: [string, string][]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of fields) out[k] = v;
  return out;
}

/** Pick a concise human summary line for an event row's fields. */
export function summaryOf(fields: Record<string, string>): string {
  return (
    fields['message'] ||
    fields['log.template'] ||
    Object.entries(fields)
      .filter(([k]) => k !== 'event.dataset')
      .slice(0, 4)
      .map(([k, v]) => `${k}=${v}`)
      .join('  ') ||
    '(no fields)'
  );
}
