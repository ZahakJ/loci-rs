import { describe, expect, it } from 'vitest';
import type { BacklogReport } from '@core/types';
import { debtSentence, paceSentence, planSentence, retentionWorkloadRatio, spreadPerDay, todayShare } from '@ui/views/backlog/plan';

const report: BacklogReport = { due: 120, overdue: 80, maxOverdueDays: 6, dailyAverage: 30, estimatedMinutes: 16, daysAtCurrentPace: 4, triggered: true };

describe('catch-up plans', () => {
  it('spreads evenly, rounding up', () => {
    expect(spreadPerDay(120, 7)).toBe(18);
    expect(spreadPerDay(10, 0)).toBe(10);
    expect(planSentence({ kind: 'spread', days: 5 }, report)).toBe('About 24 reviews a day for 5 days (3 min a day). Cards at risk come first; the safest wait their turn.');
  });
  it("today's share is the usual daily amount", () => {
    expect(todayShare(report)).toBe(30);
    expect(todayShare({ ...report, due: 12 })).toBe(12);
    expect(planSentence({ kind: 'today-share' }, report)).toBe('30 reviews today (4 min), your usual amount. The other 90 move gently forward, safest first.');
    expect(planSentence({ kind: 'today-share' }, { ...report, due: 20 })).toContain('Nothing else needs to move.');
  });
  it('lower retention quantifies the relief', () => {
    expect(retentionWorkloadRatio(0.9, 0.9)).toBeCloseTo(1, 6);
    expect(retentionWorkloadRatio(0.85, 0.9)).toBeCloseTo(0.611, 2);
    expect(planSentence({ kind: 'lower-retention', retention: 0.85, days: 7 }, report, 0.9)).toBe('For 7 days, aim to remember 85% instead of 90%. Intervals stretch a little, about 39% fewer reviews, then everything returns to normal.');
  });
  it('study everything keeps all of it', () => {
    expect(planSentence({ kind: 'study-all' }, report)).toBe('All 120 cards stay due today, about 16 min. Nothing moves.');
  });
  it('says the debt plainly', () => {
    expect(debtSentence(report)).toBe('120 cards are waiting, about 16 min of reviews.');
    expect(paceSentence(report)).toBe('The oldest has waited 6 days. At your usual 30 a day, that is 4 days of work.');
    expect(paceSentence({ ...report, maxOverdueDays: 0, dailyAverage: 0 })).toBe('');
  });
});
