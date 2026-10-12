/**
 * Every command the palette (Ctrl/Cmd+K) and the macOS menu bar can run. Menu items emit their id
 * (see `src-tauri/src/lib.rs`, `menu::FORWARDED`); the palette lists the same table. Ranking is
 * pure and tested.
 */
import { t } from '@core/i18n/i18n';
import type { IconName } from '../design/icons';
import { focusSearch, runUndo } from '../keys/useGlobalKeys';
import { navigate } from '../routes';
import { useUi } from '../store/ui';
import { MANUAL_URL, openExternal } from './native';

export interface Command {
  id: string;
  /** i18n key */
  label: string;
  /** i18n key of the group heading */
  group: 'palette.go' | 'palette.do' | 'palette.theme' | 'palette.help';
  icon: IconName;
  /** extra words the filter matches (English; not shown) */
  keywords?: string;
  /** shortcut shown at the end of the row */
  kbd?: string;
}

export const COMMANDS: Command[] = [
  { id: 'home', label: 'palette.home', group: 'palette.go', icon: 'home', keywords: 'decks today study', kbd: 'd' },
  { id: 'browse', label: 'palette.browse', group: 'palette.go', icon: 'browse', keywords: 'cards notes table', kbd: 'b' },
  { id: 'stats', label: 'palette.stats', group: 'palette.go', icon: 'stats', keywords: 'statistics charts retention forecast', kbd: 's' },
  { id: 'settings', label: 'palette.settings', group: 'palette.go', icon: 'settings', keywords: 'preferences options' },
  { id: 'backups', label: 'palette.backups', group: 'palette.go', icon: 'history', keywords: 'restore' },
  { id: 'add', label: 'palette.add', group: 'palette.do', icon: 'plus', keywords: 'new note card create', kbd: 'a' },
  { id: 'search', label: 'palette.search', group: 'palette.do', icon: 'search', keywords: 'find', kbd: '/' },
  { id: 'import', label: 'palette.import', group: 'palette.do', icon: 'import', keywords: 'anki apkg colpkg csv' },
  { id: 'undo', label: 'palette.undo', group: 'palette.do', icon: 'undo', kbd: 'ctrl+z' },
  { id: 'theme.system', label: 'palette.themeSystem', group: 'palette.theme', icon: 'system', keywords: 'appearance auto os' },
  { id: 'theme.light', label: 'palette.themeLight', group: 'palette.theme', icon: 'sun', keywords: 'appearance day' },
  { id: 'theme.dark', label: 'palette.themeDark', group: 'palette.theme', icon: 'moon', keywords: 'appearance night' },
  { id: 'shortcuts', label: 'palette.shortcuts', group: 'palette.help', icon: 'keyboard', keywords: 'keys hotkeys', kbd: '?' },
  { id: 'manual', label: 'palette.manual', group: 'palette.help', icon: 'help', keywords: 'docs documentation guide' },
];

/**
 * Commands matching `query`, best first: label prefix, then word prefix, then substring of the
 * label or keywords, then an in-order subsequence of the label. An empty query keeps table order.
 */
export function rankCommands(commands: Command[], query: string, label: (c: Command) => string): Command[] {
  const q = query.trim().toLocaleLowerCase();
  if (!q) return commands;
  const scored: { c: Command; score: number; i: number }[] = [];
  commands.forEach((c, i) => {
    const l = label(c).toLocaleLowerCase();
    const k = (c.keywords ?? '').toLocaleLowerCase();
    let score = -1;
    if (l.startsWith(q)) score = 4;
    else if (l.split(/\s+/).some((w) => w.startsWith(q))) score = 3;
    else if (l.includes(q) || k.split(/\s+/).some((w) => w.startsWith(q))) score = 2;
    else if (isSubsequence(q, l)) score = 1;
    if (score > 0) scored.push({ c, score, i });
  });
  return scored.sort((a, b) => b.score - a.score || a.i - b.i).map((s) => s.c);
}

function isSubsequence(q: string, s: string): boolean {
  let j = 0;
  for (let i = 0; i < s.length && j < q.length; i++) if (s[i] === q[j]) j++;
  return j === q.length;
}

/** Runs a command by id (from the palette or the macOS menu); unknown ids are ignored. */
export function runCommand(id: string, onAdd: () => void): void {
  const ui = useUi.getState();
  switch (id) {
    case 'home': navigate('#/'); break;
    case 'browse': navigate('#/browse'); break;
    case 'stats': navigate('#/stats/all'); break;
    case 'settings': navigate('#/settings'); break;
    case 'backups': navigate('#/backups'); break;
    case 'import': navigate('#/import'); break;
    case 'add': onAdd(); break;
    case 'search': navigate('#/browse'); focusSearch(); break;
    case 'undo': void runUndo(); break;
    case 'theme.system': ui.setThemePref('system'); break;
    case 'theme.light': ui.setThemePref('light'); break;
    case 'theme.dark': ui.setThemePref('dark'); break;
    case 'shortcuts': ui.setHelpOpen(true); break;
    case 'palette': ui.setPaletteOpen(true); break;
    case 'manual': void openExternal(MANUAL_URL); break;
    default: break;
  }
}

export const commandLabel = (c: Command): string => t(c.label);
