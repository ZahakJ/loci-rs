/**
 * Global keyboard map (pure; tested). Views own their local keys (Study, Browse, editor);
 * the global handler stays out of their way: it ignores editable targets, open modals,
 * events a view already handled (defaultPrevented), and everything except `?` on Study.
 */
import type { RouteName } from '../routes';
import { STUDY_KEYS } from '../views/study/keys';

export type GlobalAction = 'palette' | 'add' | 'browse' | 'stats' | 'home' | 'search' | 'help' | 'undo';

export interface KeyInput {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  /** focus is in a text field / contenteditable / select */
  editable: boolean;
  defaultPrevented?: boolean;
  repeat?: boolean;
}
export interface KeyContext { route: RouteName; modalOpen: boolean }

export function matchGlobal(e: KeyInput, ctx: KeyContext): GlobalAction | null {
  if (e.defaultPrevented || e.repeat || ctx.modalOpen || e.editable) return null;
  const mod = e.ctrlKey || e.metaKey;
  // Ctrl/Cmd+K opens the palette anywhere but inside a field (where the editor uses it for links).
  if (mod && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'k') return 'palette';
  if (mod && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'z') return ctx.route === 'study' ? null : 'undo';
  if (mod || e.altKey) return null;
  if (e.key === '?') return 'help';
  if (ctx.route === 'study' || ctx.route === 'welcome') return null;
  if (e.shiftKey) return null;
  switch (e.key) {
    case 'a': return 'add';
    case 'b': return 'browse';
    case 's': return 'stats';
    case 'd': return 'home';
    case '/': return 'search';
    default: return null;
  }
}

export function isEditable(el: Element | null): boolean {
  if (!el || !(el instanceof HTMLElement)) return false;
  if (el.isContentEditable) return true;
  const tag = el.tagName;
  if (tag === 'TEXTAREA' || tag === 'SELECT') return true;
  if (tag === 'INPUT') {
    const type = (el as HTMLInputElement).type;
    return !['checkbox', 'radio', 'range', 'button', 'submit', 'reset', 'color', 'file'].includes(type);
  }
  return false;
}

export interface ShortcutDef { keys: string[]; label: string }
export interface ShortcutGroup { id: string; title: string; items: ShortcutDef[] }

/** Everything the `?` overlay lists (labels are i18n keys). */
export const SHORTCUT_GROUPS: ShortcutGroup[] = [
  {
    id: 'global', title: 'keys.group.global', items: [
      { keys: ['a'], label: 'keys.add' },
      { keys: ['b'], label: 'keys.browse' },
      { keys: ['s'], label: 'keys.stats' },
      { keys: ['d'], label: 'keys.home' },
      { keys: ['/'], label: 'keys.search' },
      { keys: ['ctrl+z'], label: 'keys.undo' },
      { keys: ['ctrl+k'], label: 'keys.palette' },
      { keys: ['?'], label: 'keys.help' },
    ],
  },
  {
    // every study key, aliases included (z/u, s/@, b/-, m/*, e/o), straight from the study keymap
    id: 'study', title: 'keys.group.study', items: STUDY_KEYS.filter((k) => k.action !== 'help').map(({ keys, label }) => ({ keys, label })),
  },
  {
    id: 'edit', title: 'keys.group.edit', items: [
      { keys: ['ctrl+enter'], label: 'keys.edit.save' },
      { keys: ['ctrl+shift+c'], label: 'keys.edit.cloze' },
      { keys: ['ctrl+alt+shift+c'], label: 'keys.edit.clozeSame' },
      { keys: ['ctrl+shift+x'], label: 'keys.edit.html' },
      { keys: ['ctrl+m'], label: 'keys.edit.math' },
    ],
  },
  {
    id: 'browse', title: 'keys.group.browse', items: [
      { keys: ['j', 'k'], label: 'keys.browse.move' },
      { keys: ['shift+j', 'shift+k'], label: 'keys.browse.extend' },
      { keys: ['ctrl+a'], label: 'keys.browse.all' },
      { keys: ['/'], label: 'keys.browse.search' },
      { keys: ['i'], label: 'keys.browse.info' },
    ],
  },
];
