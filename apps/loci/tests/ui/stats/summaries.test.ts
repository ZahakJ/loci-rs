import { describe, expect, it } from 'vitest';
import { buttonShares, cardTotals, forecastSummary, hourlySummary, medianBucket, modeBucket, retentionHeadline, retentionRows, reviewSummary, rollingMean, shareFrom } from '@ui/views/stats/summaries';

describe('stats summaries', () => {
  it('forecast: averages, peak and total over the window', () => {
    const f = [{ day: 0, due: 10, withNew: 15 }, { day: 1, due: 20, withNew: 30 }, { day: 2, due: 0, withNew: 5 }, { day: 3, due: 99, withNew: 99 }];
    expect(forecastSummary(f, 3)).toEqual({ avg: 10, avgWithNew: 17, peak: 30, peakDay: 1, total: 30 });
    expect(forecastSummary([], 30).avg).toBe(0);
  });
  it('retention headline prefers 30 days and reports the gap', () => {
    const r = retentionHeadline([
      { label: '7 days', trueRetention: 0.91, desired: 0.9, reviews: 10 },
      { label: '30 days', trueRetention: 0.87, desired: 0.9, reviews: 100 },
    ])!;
    expect(r.label).toBe('30 days');
    expect(r.gap).toBeCloseTo(-0.03);
    expect(retentionHeadline([{ label: '7 days', trueRetention: null, desired: 0.9, reviews: 0 }])).toBeNull();
  });
  it('mode, median and tail share of buckets', () => {
    const b = [{ bucket: 'a', count: 1 }, { bucket: 'b', count: 5 }, { bucket: 'c', count: 4 }];
    expect(modeBucket(b)).toEqual({ bucket: 'b', share: 0.5, index: 1 });
    expect(medianBucket(b)).toBe('b');
    expect(shareFrom(b, 2)).toBeCloseTo(0.4);
    expect(modeBucket([{ bucket: 'x', count: 0 }])).toBeNull();
  });
  it('hourly: best among busy hours, busiest hour', () => {
    const h = Array.from({ length: 24 }, (_, hour) => ({ hour, reviews: hour === 3 ? 1 : hour === 21 ? 500 : 100, correctPct: hour === 3 ? 1 : hour === 9 ? 0.95 : 0.85 }));
    const s = hourlySummary(h)!;
    expect(s.best).toBe(9);
    expect(s.busiest).toBe(21);
    expect(hourlySummary(h.map((x) => ({ ...x, reviews: 0 })))).toBeNull();
  });
  it('button shares and correctness per group', () => {
    const b = buttonShares({ learn: [1, 0, 3, 0], young: [0, 0, 0, 0], mature: [10, 10, 70, 10] });
    expect(b.learn.correct).toBe(0.75);
    expect(b.young.correct).toBe(0);
    expect(b.mature.shares).toEqual([0.1, 0.1, 0.7, 0.1]);
  });
  it('card totals and review averages', () => {
    expect(cardTotals({ new: 10, learning: 0, young: 5, mature: 15, suspended: 2, buried: 1 })).toEqual({ total: 33, matureShare: 0.75, active: 30 });
    const days = [{ day: 1, count: 0, seconds: 0, learn: 0, review: 0, relearn: 0 }, { day: 2, count: 30, seconds: 600, learn: 0, review: 30, relearn: 0 }, { day: 3, count: 10, seconds: 120, learn: 0, review: 10, relearn: 0 }];
    expect(reviewSummary(days)).toEqual({ avgReviews: 20, avgMinutes: 6, studied: 2 });
    expect(rollingMean([2, 4, 6, 8], 2)).toEqual([2, 3, 5, 7]);
  });
  it('retention rows without reviews collapse into one line (N-F05)', () => {
    const r = [
      { label: '7 days', trueRetention: null, desired: 0.9, reviews: 0 },
      { label: '30 days', trueRetention: 0.88, desired: 0.9, reviews: 40 },
      { label: '1 year', trueRetention: 0, desired: 0.9, reviews: 0 },
    ];
    const { shown, hidden } = retentionRows(r);
    expect(shown.map((x) => x.label)).toEqual(['30 days']);
    expect(hidden.map((x) => x.label)).toEqual(['7 days', '1 year']);
    expect(retentionRows([]).shown).toEqual([]);
  });
});

describe('stats sentences pluralise', () => {
  it('one and many', async () => {
    const { t } = await import('@core/i18n/i18n');
    expect(t('stats.forecastSentence', { avg: 1, withNew: 1, peak: 1, day: 'Today' })).toMatch(/^About 1 review a day .* with 1 card\.$/);
    expect(t('stats.forecastSentence', { avg: 3, withNew: 4, peak: 5, day: 'Today' })).toMatch(/^About 3 reviews a day/);
    expect(t('stats.reviewsSentence', { days: 1, reviews: 1, minutes: '1 min' })).toBe('On the 1 day you studied in the last 30: about 1 review and 1 min a day.');
    expect(t('stats.reviewsSentence', { days: 4, reviews: 12, minutes: '3 min' })).toMatch(/^On the 4 days you studied in the last 30: about 12 reviews/);
    expect(t('stats.reviewsCount', { count: 1 })).toBe('1 review');
    expect(t('stats.reviewsCount', { count: 2 })).toBe('2 reviews');
    expect(t('stats.retentionHidden', { periods: '7 days' })).toBe('Not enough reviews yet for 7 days.');
    expect(t('stats.tile.learned')).toBe('New cards');
    for (const k of ['stats.forecastNone', 'stats.reviewsNone', 'stats.heatmapSummary', 'home.momentum.year']) expect(t(k, { days: 1, total: 1, best: 1, count: 1 })).not.toBe(k);
    expect(t('home.momentum.year', { count: 1 })).toMatch(/^1 review in/);
  });
});
