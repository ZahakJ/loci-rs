/** Workload forecast in words (pure; tested). Built in the UI so the copy goes through t(). */
import { t } from '@core/i18n/i18n';
import type { WorkloadForecast } from '@core/types';

const avg = (xs: number[], from: number, to: number) => {
  const s = xs.slice(from, to);
  return s.length ? s.reduce((a, b) => a + b, 0) / s.length : 0;
};

/** 'about 12 min' / 'under a minute' for a daily time in minutes. */
export function dailyTime(minutes: number): string {
  const m = Math.round(minutes);
  return m < 1 ? t('settings.forecast.underMinute') : t('settings.forecast.aboutMinutes', { count: m });
}

/**
 * 'About 41 reviews a day over the next month (about 6 min), settling near 30 a day after that.'
 * Falls back to the collection's own sentence when the series is empty.
 */
export function forecastSentence(f: Pick<WorkloadForecast, 'reviewsPerDay' | 'minutesPerDay' | 'sentence'>): string {
  if (!f.reviewsPerDay.length) return f.sentence;
  const month = avg(f.reviewsPerDay, 0, 30);
  const minutes = avg(f.minutesPerDay, 0, 30);
  const later = avg(f.reviewsPerDay, 30, 90);
  if (Math.round(month) === 0 && Math.round(later) === 0) return t('settings.forecast.none');
  return t('settings.forecast.sentence', { reviews: Math.round(month), time: dailyTime(minutes), later: Math.round(later) });
}
