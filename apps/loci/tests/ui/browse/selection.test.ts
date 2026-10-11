import { describe, expect, it } from 'vitest';
import { emptySelection, selectionReducer as R, type SelectionState } from '@ui/views/browse/selection';

const ids = [10, 20, 30, 40, 50];
const sel = (s: SelectionState) => [...s.selected].sort((a, b) => a - b);

describe('browse selection', () => {
  it('plain click selects one row and moves the cursor', () => {
    const s = R(emptySelection, { type: 'click', index: 2, ids });
    expect(sel(s)).toEqual([30]);
    expect(s.cursor).toBe(2);
  });
  it('ctrl-click toggles', () => {
    let s = R(emptySelection, { type: 'click', index: 0, ids });
    s = R(s, { type: 'click', index: 3, ids, toggle: true });
    expect(sel(s)).toEqual([10, 40]);
    s = R(s, { type: 'click', index: 0, ids, toggle: true });
    expect(sel(s)).toEqual([40]);
  });
  it('shift-click selects a range from the anchor, both directions', () => {
    let s = R(emptySelection, { type: 'click', index: 1, ids });
    s = R(s, { type: 'click', index: 3, ids, shift: true });
    expect(sel(s)).toEqual([20, 30, 40]);
    s = R(s, { type: 'click', index: 0, ids, shift: true });
    expect(sel(s)).toEqual([10, 20]);
    expect(s.anchor).toBe(1);
  });
  it('ctrl+shift adds a range to the existing selection', () => {
    let s = R(emptySelection, { type: 'click', index: 0, ids });
    s = R(s, { type: 'click', index: 3, ids, toggle: true });
    s = R(s, { type: 'click', index: 4, ids, shift: true, toggle: true });
    expect(sel(s)).toEqual([10, 40, 50]);
  });
  it('j/k move and clamp; shift extends', () => {
    let s = R(emptySelection, { type: 'move', delta: 1, ids });
    expect(s.cursor).toBe(0);
    s = R(s, { type: 'move', delta: -1, ids });
    expect(s.cursor).toBe(0);
    s = R(s, { type: 'move', delta: 1, ids, extend: true });
    s = R(s, { type: 'move', delta: 1, ids, extend: true });
    expect(sel(s)).toEqual([10, 20, 30]);
    s = R(s, { type: 'move', delta: 9, ids });
    expect(s.cursor).toBe(4);
    expect(sel(s)).toEqual([50]);
    expect(R(emptySelection, { type: 'move', delta: -1, ids }).cursor).toBe(4);
  });
  it('select all, clear and prune', () => {
    let s = R(R(emptySelection, { type: 'click', index: 4, ids }), { type: 'selectAll', ids });
    expect(s.selected.size).toBe(5);
    s = R(s, { type: 'prune', ids: [10, 20] });
    expect(sel(s)).toEqual([10, 20]);
    expect(s.cursor).toBe(1);
    s = R(s, { type: 'clear' });
    expect(s.selected.size).toBe(0);
    expect(R(s, { type: 'prune', ids: [10, 20] })).toBe(s);
  });
});
