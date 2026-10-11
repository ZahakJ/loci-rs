/**
 * Catch-up plan arithmetic and sentences (DESIGN §9). Pure (tested): every plan card says in one
 * sentence what will happen, in reviews and minutes, before anything changes.
 */
import { t } from '@core/i18n/i18n';
import type { BacklogReport, CatchUpPlan } from '@core/types';
import { formatMinutes } from '../../hooks/format';

export const SPREAD_DAYS = [3, 5, 7, 14] as const;
export const LOWER_RETENTION_DAYS = 7;

/** Seconds per review from the report's own estimate (8 s when there is nothing to go on). */
export function secondsPerCard(r: BacklogReport): number {
  return r.due > 0 && r.estimatedMinutes > 0 ? (r.estimatedMinutes * 60) / r.due : 8;
}

export function minutesFor(cards: number, r: BacklogReport): number {
  return (cards * secondsPerCard(r)) / 60;
}

/** Reviews per day when the backlog is spread over `days` (on top of nothing else). */
export function spreadPerDay(due: number, days: number): number {
  return days > 0 ? Math.ceil(due / days) : due;
}

/** Today's usual share: the recent daily average, never more than what is due. */
export function todayShare(r: BacklogReport): number {
  return Math.min(r.due, Math.max(1, Math.round(r.dailyAverage)));
}

/**
 * Relative review load at retention `target` versus `current` (FSRS power curve, decay −0.5):
 * intervals scale with (R^(−2) − 1), so the load scales with its inverse. 0.85 vs 0.90 ≈ 0.61.
 */
export function retentionWorkloadRatio(target: number, current: number): number {
  const k = (r: number) => Math.pow(Math.min(0.999, Math.max(0.5, r)), -2) - 1;
  return k(current) / k(target);
}

const pct = (x: number) => `${Math.round(x * 100)}%`;

/** One sentence describing what a plan does to this backlog. */
export function planSentence(plan: CatchUpPlan, r: BacklogReport, currentRetention = 0.9): string {
  switch (plan.kind) {
    case 'spread': {
      const perDay = spreadPerDay(r.due, plan.days);
      return t('catchUpView.sentence.spread', { perDay, days: plan.days, duration: formatMinutes(minutesFor(perDay, r)) });
    }
    case 'today-share': {
      const share = todayShare(r);
      return t('catchUpView.sentence.todayShare', { count: share, duration: formatMinutes(minutesFor(share, r)), rest: Math.max(0, r.due - share) });
    }
    case 'lower-retention': {
      const ratio = retentionWorkloadRatio(plan.retention, currentRetention);
      const fewer = Math.max(0, Math.round((1 - ratio) * 100));
      return t('catchUpView.sentence.lowerRetention', { days: plan.days, target: pct(plan.retention), current: pct(currentRetention), fewer });
    }
    case 'study-all':
      return t('catchUpView.sentence.studyAll', { count: r.due, duration: formatMinutes(r.estimatedMinutes) });
  }
}

/** The headline: how big the debt is, in cards and minutes. */
export function debtSentence(r: BacklogReport): string {
  return t('catchUp.debt', { count: r.due, duration: formatMinutes(r.estimatedMinutes) });
}

/** The quieter second line: how old, and what the usual pace means. */
export function paceSentence(r: BacklogReport): string {
  const parts: string[] = [];
  if (r.maxOverdueDays > 0) parts.push(t('catchUpView.oldest', { count: r.maxOverdueDays }));
  if (r.dailyAverage > 0) parts.push(t('catchUpView.pace', { average: Math.round(r.dailyAverage), days: Math.max(1, r.daysAtCurrentPace) }));
  return parts.join(' ');
}
