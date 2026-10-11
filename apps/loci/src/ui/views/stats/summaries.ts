/**
 * Numbers behind each Stats panel's one sentence (pure; tested). The view turns them into words
 * with t(); keeping the arithmetic here makes the sentences testable and consistent.
 */
import type { Stats } from '@core/types';

export function forecastSummary(forecast: Stats['forecast'], days: number): { avg: number; avgWithNew: number; peak: number; peakDay: number; total: number } {
  const slice = forecast.slice(0, days);
  if (!slice.length) return { avg: 0, avgWithNew: 0, peak: 0, peakDay: 0, total: 0 };
  const total = slice.reduce((a, d) => a + d.due, 0);
  const totalNew = slice.reduce((a, d) => a + d.withNew, 0);
  let peak = -1, peakDay = 0;
  for (const d of slice) if (d.withNew > peak) { peak = d.withNew; peakDay = d.day; }
  return { avg: Math.round(total / slice.length), avgWithNew: Math.round(totalNew / slice.length), peak, peakDay, total };
}

/** The period used for the headline (30 days if it has reviews, else the longest with data). */
export function retentionHeadline(retention: Stats['retention']): { label: string; actual: number; target: number; gap: number; reviews: number } | null {
  const withData = retention.filter((r) => r.trueRetention !== null && r.reviews > 0);
  if (!withData.length) return null;
  const pick = withData.find((r) => /30/.test(r.label)) ?? withData[withData.length - 1]!;
  const actual = pick.trueRetention!;
  return { label: pick.label, actual, target: pick.desired, gap: actual - pick.desired, reviews: pick.reviews };
}

/** Retention rows worth drawing (some reviews behind them) and the periods without any. */
export function retentionRows(retention: Stats['retention']): { shown: Stats['retention']; hidden: Stats['retention'] } {
  const shown = retention.filter((r) => r.reviews > 0 && r.trueRetention !== null);
  return { shown, hidden: retention.filter((r) => !shown.includes(r)) };
}

/** Bucket holding the most cards and its share of all cards. */
export function modeBucket(buckets: Array<{ bucket: string; count: number }>): { bucket: string; share: number; index: number } | null {
  const total = buckets.reduce((a, b) => a + b.count, 0);
  if (!total) return null;
  let index = 0;
  buckets.forEach((b, i) => { if (b.count > buckets[index]!.count) index = i; });
  return { bucket: buckets[index]!.bucket, share: buckets[index]!.count / total, index };
}

/** Bucket containing the median card. */
export function medianBucket(buckets: Array<{ bucket: string; count: number }>): string | null {
  const total = buckets.reduce((a, b) => a + b.count, 0);
  if (!total) return null;
  let acc = 0;
  for (const b of buckets) { acc += b.count; if (acc >= total / 2) return b.bucket; }
  return buckets[buckets.length - 1]!.bucket;
}

/** Share of cards in buckets from `from` (inclusive) to the end. */
export function shareFrom(buckets: Array<{ count: number }>, from: number): number {
  const total = buckets.reduce((a, b) => a + b.count, 0);
  if (!total) return 0;
  return buckets.slice(from).reduce((a, b) => a + b.count, 0) / total;
}

/** Best-remembered hour (among hours with enough reviews) and the busiest hour. */
export function hourlySummary(hourly: Stats['hourly'], minShare = 0.03): { best: number; bestPct: number; busiest: number; worst: number; worstPct: number } | null {
  const total = hourly.reduce((a, h) => a + h.reviews, 0);
  if (!total) return null;
  const enough = hourly.filter((h) => h.reviews >= total * minShare);
  const pool = enough.length ? enough : hourly.filter((h) => h.reviews > 0);
  const best = pool.reduce((a, h) => (h.correctPct > a.correctPct ? h : a), pool[0]!);
  const worst = pool.reduce((a, h) => (h.correctPct < a.correctPct ? h : a), pool[0]!);
  const busiest = hourly.reduce((a, h) => (h.reviews > a.reviews ? h : a), hourly[0]!);
  return { best: best.hour, bestPct: best.correctPct, busiest: busiest.hour, worst: worst.hour, worstPct: worst.correctPct };
}

/** Share of each button within each maturity group; correct = 1 − again share. */
export function buttonShares(buttons: Stats['buttons']): Record<'learn' | 'young' | 'mature', { shares: number[]; total: number; correct: number }> {
  const one = (arr: number[]) => {
    const total = arr.reduce((a, b) => a + b, 0);
    const shares = arr.map((v) => (total ? v / total : 0));
    return { shares, total, correct: total ? 1 - (shares[0] ?? 0) : 0 };
  };
  return { learn: one(buttons.learn), young: one(buttons.young), mature: one(buttons.mature) };
}

export function cardTotals(c: Stats['cardCounts']): { total: number; matureShare: number; active: number } {
  const total = c.new + c.learning + c.young + c.mature + c.suspended + c.buried;
  const studied = c.learning + c.young + c.mature;
  return { total, matureShare: studied ? c.mature / studied : 0, active: total - c.suspended - c.buried };
}

/** Per-day averages over days with any review in the last `window` days. */
export function reviewSummary(days: Stats['reviewsPerDay'], window = 30): { avgReviews: number; avgMinutes: number; studied: number } {
  const recent = days.slice(-window);
  const studied = recent.filter((d) => d.count > 0);
  if (!studied.length) return { avgReviews: 0, avgMinutes: 0, studied: 0 };
  return {
    avgReviews: Math.round(studied.reduce((a, d) => a + d.count, 0) / studied.length),
    avgMinutes: Math.round(studied.reduce((a, d) => a + d.seconds, 0) / studied.length / 60),
    studied: studied.length,
  };
}

/** Rolling mean (trailing window) used to smooth minutes per day. */
export function rollingMean(values: number[], window: number): number[] {
  const out: number[] = [];
  let sum = 0;
  for (let i = 0; i < values.length; i++) {
    sum += values[i]!;
    if (i >= window) sum -= values[i - window]!;
    out.push(sum / Math.min(i + 1, window));
  }
  return out;
}
