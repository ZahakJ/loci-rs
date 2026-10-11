import { describe, expect, it } from 'vitest';
import { oneByOneInit, oneByOnePending, oneByOneReducer } from '@ui/card/oneByOne';

describe('one-by-one reveal', () => {
  it('reveals one cloze per step, then stops', () => {
    let s = oneByOneInit(3);
    expect(oneByOnePending(s)).toBe(true);
    s = oneByOneReducer(s, { type: 'advance' });
    s = oneByOneReducer(s, { type: 'advance' });
    expect(s.revealed).toBe(2);
    expect(oneByOnePending(s)).toBe(true);
    s = oneByOneReducer(s, { type: 'advance' });
    expect(oneByOnePending(s)).toBe(false);
    expect(oneByOneReducer(s, { type: 'advance' })).toBe(s);
  });
  it('is never pending without clozes', () => {
    expect(oneByOnePending(oneByOneInit(0))).toBe(false);
  });
  it('clicking a cloze reveals up to it, never backwards', () => {
    let s = oneByOneInit(4);
    s = oneByOneReducer(s, { type: 'reveal', index: 2 });
    expect(s.revealed).toBe(3);
    expect(oneByOneReducer(s, { type: 'reveal', index: 0 })).toBe(s);
    expect(oneByOneReducer(s, { type: 'reveal', index: 9 }).revealed).toBe(4);
  });
  it('reset and reveal-all', () => {
    const s = oneByOneReducer(oneByOneInit(2), { type: 'reveal-all' });
    expect(s.revealed).toBe(2);
    expect(oneByOneReducer(s, { type: 'reset', total: 5 })).toEqual({ total: 5, revealed: 0 });
  });
});
