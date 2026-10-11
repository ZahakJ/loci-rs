import { describe, expect, it } from 'vitest';
import { listKey, step, typeahead, type NavOption, type Typeahead } from '../../../src/ui/design/primitives/listNav';

const opts: NavOption[] = [
  { label: 'Midnight' }, { label: '1 AM' }, { label: '2 AM', disabled: true }, { label: '3 AM' }, { label: '4 AM' },
  { label: 'Morning' }, { label: 'Mid-morning' },
];
const fresh = (): Typeahead => ({ text: '', at: 0 });

describe('Select keyboard model', () => {
  it('arrows move and skip disabled options, stopping at the ends', () => {
    expect(listKey(opts, 1, 'ArrowDown', fresh(), 0)).toEqual({ kind: 'move', to: 3 });
    expect(listKey(opts, 3, 'ArrowUp', fresh(), 0)).toEqual({ kind: 'move', to: 1 });
    expect(listKey(opts, 6, 'ArrowDown', fresh(), 0)).toEqual({ kind: 'move', to: 6 });
    expect(listKey(opts, 0, 'ArrowUp', fresh(), 0)).toEqual({ kind: 'move', to: 0 });
    expect(step(opts, 1, 1)).toBe(3);
  });

  it('Home and End jump to the first and last enabled option', () => {
    const edge: NavOption[] = [{ label: 'a', disabled: true }, { label: 'b' }, { label: 'c' }, { label: 'd', disabled: true }];
    expect(listKey(edge, 2, 'Home', fresh(), 0)).toEqual({ kind: 'move', to: 1 });
    expect(listKey(edge, 1, 'End', fresh(), 0)).toEqual({ kind: 'move', to: 2 });
  });

  it('Enter and Space pick the active option; never a disabled one', () => {
    expect(listKey(opts, 3, 'Enter', fresh(), 0)).toEqual({ kind: 'pick', index: 3 });
    expect(listKey(opts, 4, ' ', fresh(), 0)).toEqual({ kind: 'pick', index: 4 });
    expect(listKey(opts, 2, 'Enter', fresh(), 0)).toEqual({ kind: 'none' });
    expect(listKey(opts, 1, 'ArrowUp', fresh(), 0, { alt: true })).toEqual({ kind: 'pick', index: 1 });
  });

  it('Escape closes, Tab leaves', () => {
    expect(listKey(opts, 1, 'Escape', fresh(), 0)).toEqual({ kind: 'close' });
    expect(listKey(opts, 1, 'Tab', fresh(), 0)).toEqual({ kind: 'tab' });
    expect(listKey(opts, 1, 'F5', fresh(), 0)).toEqual({ kind: 'none' });
  });

  it('typeahead builds a prefix within 600 ms and cycles on a repeated letter', () => {
    const ta = fresh();
    expect(typeahead(opts, 0, 'm', ta, 1000)).toBe(5); // next "M…" after the active one
    expect(typeahead(opts, 5, 'i', ta, 1200)).toBe(6); // "mi" → Mid-morning (Morning no longer matches)
    const cyc = fresh();
    expect(typeahead(opts, 0, 'm', cyc, 5000)).toBe(5);
    expect(typeahead(opts, 5, 'm', cyc, 5100)).toBe(6);
    expect(typeahead(opts, 6, 'm', cyc, 5200)).toBe(0);
    expect(typeahead(opts, 0, '2', fresh(), 9000)).toBe(0); // disabled "2 AM" is skipped; nothing else matches
    expect(listKey(opts, 0, '3', fresh(), 0)).toEqual({ kind: 'move', to: 3 });
  });

  it('Space continues a typeahead in progress instead of picking', () => {
    const ta = fresh();
    expect(listKey(opts, 0, '4', ta, 100)).toEqual({ kind: 'move', to: 4 });
    expect(listKey(opts, 4, ' ', ta, 200)).toEqual({ kind: 'move', to: 4 });
    expect(listKey(opts, 4, ' ', ta, 2000)).toEqual({ kind: 'pick', index: 4 });
  });
});
