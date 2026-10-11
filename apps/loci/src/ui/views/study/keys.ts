/**
 * Study-screen keyboard map (DESIGN §8; Anki veteran keys from research/anki-complaints.md §4).
 * Pure and tested: `matchStudyKey` turns a key event plus context into one action.
 */
import type { Flag, Rating } from '@core/types';

export type StudyPhase = 'front' | 'back' | 'other';

export type StudyAction =
  | { type: 'reveal' }
  | { type: 'grade'; rating: Rating }
  | { type: 'undo' }
  | { type: 'edit' }
  | { type: 'suspend' }
  | { type: 'suspend-note' }
  | { type: 'bury' }
  | { type: 'bury-note' }
  | { type: 'delete-note' }
  | { type: 'set-due' }
  | { type: 'flag-toggle' }
  | { type: 'flag'; flag: Flag }
  | { type: 'mark' }
  | { type: 'info' }
  | { type: 'replay' }
  | { type: 'type-focus' }
  | { type: 'help' }
  | { type: 'leave' };

export interface StudyKeyInput {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  repeat?: boolean;
  /** focus is in a text field (the typed-answer box included) */
  editable: boolean;
  /** focus is on a button, link or other control that handles Enter/Space itself */
  interactive?: boolean;
}

/** One row of the `?` overlay / docs: keys and the i18n key of the label. */
export interface StudyKeyDef { keys: string[]; label: string; action: StudyAction['type'] }

/** Every study key, in the order the help overlay lists them (DESIGN §8). */
export const STUDY_KEYS: StudyKeyDef[] = [
  { keys: ['space', 'enter'], label: 'keys.study.show', action: 'reveal' },
  { keys: ['1', '2', '3', '4'], label: 'keys.study.grade', action: 'grade' },
  { keys: ['z', 'u', 'ctrl+z'], label: 'keys.study.undo', action: 'undo' },
  { keys: ['e', 'o'], label: 'keys.study.edit', action: 'edit' },
  { keys: ['s', '@'], label: 'keys.study.suspend', action: 'suspend' },
  { keys: ['!'], label: 'keys.study.suspendNote', action: 'suspend-note' },
  { keys: ['b', '-'], label: 'keys.study.bury', action: 'bury' },
  { keys: ['='], label: 'keys.study.buryNote', action: 'bury-note' },
  { keys: ['f'], label: 'keys.study.flag', action: 'flag-toggle' },
  { keys: ['ctrl+1', 'ctrl+7'], label: 'keys.study.flagColour', action: 'flag' },
  { keys: ['m', '*'], label: 'keys.study.mark', action: 'mark' },
  { keys: ['ctrl+shift+d'], label: 'keys.study.setDue', action: 'set-due' },
  { keys: ['ctrl+delete'], label: 'keys.study.deleteNote', action: 'delete-note' },
  { keys: ['i'], label: 'keys.study.info', action: 'info' },
  { keys: ['r'], label: 'keys.study.replay', action: 'replay' },
  { keys: ['t'], label: 'keys.study.type', action: 'type-focus' },
  { keys: ['?'], label: 'keys.help', action: 'help' },
  { keys: ['esc'], label: 'keys.study.leave', action: 'leave' },
];

const GRADE_KEYS: Record<string, Rating> = { '1': 1, '2': 2, '3': 3, '4': 4 };

/** Maps a keydown to a study action, or null when the key is not ours in this context. */
export function matchStudyKey(e: StudyKeyInput, phase: StudyPhase): StudyAction | null {
  if (e.editable || e.repeat) return null;
  const mod = e.ctrlKey || e.metaKey;
  const key = e.key;

  if (mod && !e.altKey) {
    if (!e.shiftKey && key.toLowerCase() === 'z') return { type: 'undo' };
    if (phase !== 'other' && !e.shiftKey && (key === 'Delete' || (key === 'Backspace' && e.metaKey))) return { type: 'delete-note' };
    if (phase !== 'other' && e.shiftKey && key.toLowerCase() === 'd') return { type: 'set-due' };
    // ctrl+1..7 set a flag colour; ctrl+0 clears (Anki uses the same digit to toggle off).
    if (!e.shiftKey && /^[0-7]$/.test(key) && phase !== 'other') return { type: 'flag', flag: Number(key) as Flag };
    return null;
  }
  if (e.altKey) return null;

  if (key === 'Escape') return { type: 'leave' };
  if (key === '?') return { type: 'help' };

  if (key === ' ' || key === 'Enter') {
    if (e.interactive || e.shiftKey) return null;
    if (phase === 'front') return { type: 'reveal' };
    if (phase === 'back') return { type: 'grade', rating: 3 };
    return null;
  }

  const rating = GRADE_KEYS[key];
  if (rating !== undefined) return phase === 'back' && !e.shiftKey ? { type: 'grade', rating } : null;

  if (key === 'z' || key === 'u') return e.shiftKey ? null : { type: 'undo' };
  if (phase === 'other') return null;

  // Symbol keys from Anki need shift on most layouts, so shift is allowed for them.
  switch (key) {
    case '@': return { type: 'suspend' };
    case '!': return { type: 'suspend-note' };
    case '-': return { type: 'bury' };
    case '=': return { type: 'bury-note' };
    case '*': return { type: 'mark' };
  }
  if (e.shiftKey) return null;
  switch (key) {
    case 'e': case 'o': return { type: 'edit' };
    case 's': return { type: 'suspend' };
    case 'b': return { type: 'bury' };
    case 'f': return { type: 'flag-toggle' };
    case 'm': return { type: 'mark' };
    case 'i': return { type: 'info' };
    case 'r': return { type: 'replay' };
    case 't': return { type: 'type-focus' };
    default: return null;
  }
}

/** True when an element handles Enter/Space itself (so the study keys leave them alone). */
export function isInteractiveElement(el: Element | null): boolean {
  if (!el) return false;
  if (el.closest('[role="menu"], [role="dialog"], [role="alertdialog"]')) return true;
  const tag = el.tagName;
  if (tag === 'BUTTON' || tag === 'A' || tag === 'SUMMARY' || tag === 'SELECT') return true;
  const role = el.getAttribute('role');
  return role === 'button' || role === 'menuitem' || role === 'radio' || role === 'checkbox' || role === 'switch' || role === 'tab' || role === 'link';
}
