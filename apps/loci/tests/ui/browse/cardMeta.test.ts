import { describe, expect, it } from 'vitest';
import { CardQueue, CardType, type Card } from '@core/types';
import { cardMetaParts } from '@ui/views/browse/cardMeta';

const base: Card = {
  id: 1, nid: 1, did: 1, ord: 0, mtime: 0, usn: 0, type: CardType.Review, queue: CardQueue.Review, due: 100, ivl: 12,
  factor: 2500, reps: 7, lapses: 1, left: 0, odue: 0, odid: 0, flags: 0, data: {},
};
const now = 1_800_000_000;

describe('Browse editor facts line (V-F05)', () => {
  it('review card: deck, due, interval, ease, reviews, lapses', () => {
    expect(cardMetaParts(base, { dueDate: now + 3 * 86400, difficulty: null }, 'Lang::French', now))
      .toEqual(['Lang / French', 'due in 3 days', 'interval 12d', 'ease 250%', '7 reviews', '1 lapse']);
  });
  it('FSRS difficulty wins over ease', () => {
    const parts = cardMetaParts({ ...base, data: { d: 5.5 } }, { dueDate: now - 86400, difficulty: 5.5 }, undefined, now);
    expect(parts).toContain('difficulty 50%');
    expect(parts).toContain('due yesterday');
    expect(parts.some((p) => p.startsWith('ease'))).toBe(false);
  });
  it('new card: position and no lapses', () => {
    expect(cardMetaParts({ ...base, type: CardType.New, queue: CardQueue.New, due: 4, ivl: 0, reps: 0, lapses: 0 }, null, 'Default', now))
      .toEqual(['Default', 'New #4', 'no reviews']);
  });
  it('suspended cards say so', () => {
    expect(cardMetaParts({ ...base, queue: CardQueue.Suspended }, null, undefined, now)[0]).toBe('Suspended');
  });
});
