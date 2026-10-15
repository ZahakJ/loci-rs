import { describe, expect, it } from 'vitest';
import { STUDY_KEYS, matchStudyKey, type StudyKeyInput } from '@ui/views/study/keys';

const k = (key: string, extra: Partial<StudyKeyInput> = {}): StudyKeyInput => ({ key, ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, editable: false, ...extra });

describe('matchStudyKey', () => {
  it('space and enter show the answer on the front and mean Good on the back', () => {
    expect(matchStudyKey(k(' '), 'front')).toEqual({ type: 'reveal' });
    expect(matchStudyKey(k('Enter'), 'front')).toEqual({ type: 'reveal' });
    expect(matchStudyKey(k(' '), 'back')).toEqual({ type: 'grade', rating: 3 });
    expect(matchStudyKey(k('Enter'), 'back')).toEqual({ type: 'grade', rating: 3 });
  });
  it('1-4 grade only on the back', () => {
    for (const r of [1, 2, 3, 4]) expect(matchStudyKey(k(String(r)), 'back')).toEqual({ type: 'grade', rating: r });
    expect(matchStudyKey(k('1'), 'front')).toBeNull();
    expect(matchStudyKey(k('5'), 'back')).toBeNull();
  });
  it('leaves Enter/Space to focused buttons', () => {
    expect(matchStudyKey(k(' ', { interactive: true }), 'back')).toBeNull();
    expect(matchStudyKey(k('Enter', { interactive: true }), 'front')).toBeNull();
    expect(matchStudyKey(k('3', { interactive: true }), 'back')).toEqual({ type: 'grade', rating: 3 });
  });
  it('ignores typing, repeats and alt', () => {
    expect(matchStudyKey(k('e', { editable: true }), 'front')).toBeNull();
    expect(matchStudyKey(k(' ', { repeat: true }), 'front')).toBeNull();
    expect(matchStudyKey(k('e', { altKey: true }), 'front')).toBeNull();
  });
  it('maps the internals §6 letters', () => {
    expect(matchStudyKey(k('z'), 'front')).toEqual({ type: 'undo' });
    expect(matchStudyKey(k('e'), 'back')).toEqual({ type: 'edit' });
    expect(matchStudyKey(k('s'), 'front')).toEqual({ type: 'suspend' });
    expect(matchStudyKey(k('b'), 'front')).toEqual({ type: 'bury' });
    expect(matchStudyKey(k('f'), 'front')).toEqual({ type: 'flag-toggle' });
    expect(matchStudyKey(k('m'), 'front')).toEqual({ type: 'mark' });
    expect(matchStudyKey(k('i'), 'back')).toEqual({ type: 'info' });
    expect(matchStudyKey(k('r'), 'front')).toEqual({ type: 'replay' });
    expect(matchStudyKey(k('t'), 'front')).toEqual({ type: 'type-focus' });
    expect(matchStudyKey(k('?', { shiftKey: true }), 'front')).toEqual({ type: 'help' });
    expect(matchStudyKey(k('Escape'), 'other')).toEqual({ type: 'leave' });
  });
  it('accepts the Anki veteran aliases', () => {
    expect(matchStudyKey(k('u'), 'back')).toEqual({ type: 'undo' });
    expect(matchStudyKey(k('@', { shiftKey: true }), 'front')).toEqual({ type: 'suspend' });
    expect(matchStudyKey(k('-'), 'front')).toEqual({ type: 'bury' });
    expect(matchStudyKey(k('*', { shiftKey: true }), 'front')).toEqual({ type: 'mark' });
  });
  it('note-level and Anki extras: = ! @ o ctrl+delete ctrl+shift+d', () => {
    expect(matchStudyKey(k('=', {}), 'back')).toEqual({ type: 'bury-note' });
    expect(matchStudyKey(k('!', { shiftKey: true }), 'front')).toEqual({ type: 'suspend-note' });
    expect(matchStudyKey(k('@', { shiftKey: true }), 'back')).toEqual({ type: 'suspend' });
    expect(matchStudyKey(k('o'), 'front')).toEqual({ type: 'edit' });
    expect(matchStudyKey(k('Delete', { ctrlKey: true }), 'back')).toEqual({ type: 'delete-note' });
    expect(matchStudyKey(k('Backspace', { metaKey: true }), 'front')).toEqual({ type: 'delete-note' });
    expect(matchStudyKey(k('Backspace', { ctrlKey: true }), 'front')).toBeNull();
    expect(matchStudyKey(k('D', { ctrlKey: true, shiftKey: true }), 'front')).toEqual({ type: 'set-due' });
    expect(matchStudyKey(k('Delete', { ctrlKey: true }), 'other')).toBeNull();
    expect(matchStudyKey(k('=', {}), 'other')).toBeNull();
  });
  it('typing in the answer box is never a shortcut (space types a space)', () => {
    for (const key of [' ', '1', '3', 'e', '=', 'Enter']) expect(matchStudyKey(k(key, { editable: true }), 'front')).toBeNull();
  });
  it('ctrl+1..7 set flags, ctrl+z undoes', () => {
    for (let f = 1; f <= 7; f++) expect(matchStudyKey(k(String(f), { ctrlKey: true }), 'front')).toEqual({ type: 'flag', flag: f });
    expect(matchStudyKey(k('0', { ctrlKey: true }), 'back')).toEqual({ type: 'flag', flag: 0 });
    expect(matchStudyKey(k('8', { ctrlKey: true }), 'back')).toBeNull();
    expect(matchStudyKey(k('z', { metaKey: true }), 'other')).toEqual({ type: 'undo' });
  });
  it('shifted letters are not shortcuts', () => {
    expect(matchStudyKey(k('S', { shiftKey: true }), 'front')).toBeNull();
  });
  it('outside a card only undo, help and leave work', () => {
    expect(matchStudyKey(k('e'), 'other')).toBeNull();
    expect(matchStudyKey(k(' '), 'other')).toBeNull();
    expect(matchStudyKey(k('z'), 'other')).toEqual({ type: 'undo' });
  });
  it('documents every action it can return', () => {
    const actions = new Set(STUDY_KEYS.map((d) => d.action));
    for (const a of ['reveal', 'grade', 'undo', 'edit', 'suspend', 'suspend-note', 'bury', 'bury-note', 'delete-note', 'set-due', 'flag-toggle', 'flag', 'mark', 'info', 'replay', 'type-focus', 'help', 'leave']) expect(actions.has(a as never)).toBe(true);
  });
});
