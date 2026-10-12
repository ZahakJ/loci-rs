import { describe, expect, it, vi } from 'vitest';

vi.mock('@ui/keys/useGlobalKeys', () => ({ focusSearch: () => {}, runUndo: async () => {} }));
vi.mock('@ui/routes', () => ({ navigate: () => {} }));
vi.mock('@ui/store/ui', () => ({ useUi: { getState: () => ({}) } }));
vi.mock('@ui/shell/native', () => ({ MANUAL_URL: '', openExternal: async () => {} }));
vi.mock('@core/i18n/i18n', () => ({ t: (k: string) => k }));

const { COMMANDS, rankCommands } = await import('@ui/shell/commands');
const LABELS: Record<string, string> = {
  home: 'Home', browse: 'Browse', stats: 'Statistics', settings: 'Settings', backups: 'Backups', add: 'Add a note',
  search: 'Search cards', import: 'Import a deck or collection', undo: 'Undo', 'theme.system': 'Theme: match the system',
  'theme.light': 'Theme: light', 'theme.dark': 'Theme: dark', shortcuts: 'Keyboard shortcuts', manual: 'Open the manual',
};
const label = (c: { id: string }) => LABELS[c.id] ?? c.id;
const ids = (q: string) => rankCommands(COMMANDS, q, label).map((c) => c.id);

describe('rankCommands', () => {
  it('keeps table order for an empty query', () => {
    expect(ids('  ')).toEqual(COMMANDS.map((c) => c.id));
  });
  it('ranks a label prefix before a word prefix before a keyword', () => {
    expect(ids('st')[0]).toBe('stats');
    expect(ids('dark')).toEqual(['theme.dark']);
    expect(ids('night')).toEqual(['theme.dark']);
    expect(ids('apkg')).toEqual(['import']);
  });
  it('falls back to an in-order subsequence', () => {
    expect(ids('kbsh')).toContain('shortcuts');
    expect(ids('zzz')).toEqual([]);
  });
  it('every command has a unique id and an icon', () => {
    expect(new Set(COMMANDS.map((c) => c.id)).size).toBe(COMMANDS.length);
    for (const c of COMMANDS) expect(c.icon).toBeTruthy();
  });
});
