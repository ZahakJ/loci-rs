/** Pure helpers for the study screen (tested): grade labels, announcements, custom-session parsing. */
import { t } from '@core/i18n/i18n';
import { formatInterval } from '@core/scheduler/convert';
import type { Rating, SchedulingPreview, SessionSummary, StudyCounts } from '@core/types';

export const RATINGS: readonly Rating[] = [1, 2, 3, 4];
export type GradeKey = 'again' | 'hard' | 'good' | 'easy';
export const GRADE_KEYS: Record<Rating, GradeKey> = { 1: 'again', 2: 'hard', 3: 'good', 4: 'easy' };

export function gradeName(r: Rating): string { return t(`study.${GRADE_KEYS[r]}`); }

/**
 * The short interval label under a grade button ('<10m', '4d', '1.2mo').
 * Uses the scheduler's label; falls back to formatting `secondsUntil`; '' when neither exists.
 */
export function gradeLabel(preview: SchedulingPreview | null | undefined, r: Rating): string {
  if (!preview) return '';
  const label = preview.labels?.[r];
  if (label) return label;
  const secs = preview.secondsUntil?.[r];
  return typeof secs === 'number' && Number.isFinite(secs) ? formatInterval(secs, 60) : '';
}

/** Spoken interval in words: 'under a minute', '10 minutes', '3 hours', '4 days', '2 months', '1.5 years'. */
export function intervalWords(seconds: number): string {
  const s = Math.max(0, seconds);
  if (s < 60) return t('studyScreen.interval.lessThanMinute');
  if (s < 3600) return t('studyScreen.interval.minutes', { count: Math.round(s / 60) });
  if (s < 86400) return t('studyScreen.interval.hours', { count: Math.round(s / 3600) });
  const days = s / 86400;
  if (days < 30) return t('studyScreen.interval.days', { count: Math.round(days) });
  if (days < 365) return t('studyScreen.interval.months', { count: Math.round((days / 30) * 10) / 10 });
  return t('studyScreen.interval.years', { count: Math.round((days / 365) * 10) / 10 });
}

/**
 * A grade-button label ('6m', '<10m', '3d', '1.2mo', '2y', '45s') in words for screen readers, so
 * what is announced is exactly what the button showed. Null when the label is not in that form.
 */
export function labelWords(label: string): string | null {
  const m = /^(<)?\s*(\d+(?:[.,]\d+)?)\s*(s|m|h|d|w|mo|y)$/i.exec(label.trim());
  if (!m) return null;
  const n = Number(m[2]!.replace(',', '.'));
  const unit = m[3]!.toLowerCase();
  const key = unit === 's' ? 'seconds' : unit === 'm' ? 'minutes' : unit === 'h' ? 'hours' : unit === 'd' ? 'days' : unit === 'w' ? 'weeks' : unit === 'mo' ? 'months' : 'years';
  if (key === 'seconds' && !m[1] && n < 60) return t('studyScreen.interval.lessThanMinute');
  const words = t(`studyScreen.interval.${key}`, { count: n });
  return m[1] ? t('studyScreen.interval.under', { interval: words }) : words;
}

/**
 * Live-region text after grading: 'Graded Good, next in 6 minutes'. Built from the same label the
 * grade button showed (gradeLabel), so the two never disagree; falls back to secondsUntil.
 */
export function gradedAnnouncement(r: Rating, preview: SchedulingPreview | null | undefined): string {
  const label = gradeLabel(preview, r);
  const fromLabel = label ? labelWords(label) : null;
  if (fromLabel) return t('studyScreen.graded', { grade: gradeName(r), interval: fromLabel });
  const secs = preview?.secondsUntil?.[r];
  if (typeof secs !== 'number' || !Number.isFinite(secs)) return t('studyScreen.gradedPlain', { grade: gradeName(r) });
  return t('studyScreen.graded', { grade: gradeName(r), interval: intervalWords(secs) });
}

/** Anki's set-due-date syntax: '0', '3', '1!', '3-7', '3-7!'. */
export function isDueSyntax(text: string): boolean {
  const m = /^(\d{1,5})(?:-(\d{1,5}))?(!)?$/.exec(text.trim());
  return !!m && (m[2] === undefined || Number(m[2]) >= Number(m[1]));
}

/** How many more new cards to offer in the 'nothing due' state: the day's allowance, capped by what is left. */
export function moreNewOffer(newRemaining: number, newPerDay: number): number {
  if (!(newRemaining > 0)) return 0;
  const step = newPerDay > 0 ? newPerDay : 10;
  return Math.min(Math.floor(newRemaining), step);
}

/** 'Undo Good' → 'Good' (labels from the collection may or may not carry the verb). */
export function undoSubject(label: string): string {
  return label.replace(/^undo\s+/i, '').trim() || label;
}

export function totalRemaining(c: StudyCounts | null | undefined): number {
  return c ? c.new + c.learn + c.review : 0;
}

/** Session progress 0..1 from answers given and cards still queued. */
export function sessionProgress(done: number, remaining: number): number {
  const total = done + remaining;
  return total <= 0 ? 0 : Math.min(1, done / total);
}

/** Grade shares for the summary bar; zero-total sessions give all zeros. */
export function gradeShares(s: Pick<SessionSummary, 'again' | 'hard' | 'good' | 'easy'>): Record<GradeKey, number> {
  const total = s.again + s.hard + s.good + s.easy;
  if (total <= 0) return { again: 0, hard: 0, good: 0, easy: 0 };
  return { again: s.again / total, hard: s.hard / total, good: s.good / total, easy: s.easy / total };
}

/** Recorded answer time: elapsed ms capped at the preset's maxAnswerSeconds (0 = no cap). */
export function cappedElapsed(elapsedMs: number, maxAnswerSeconds: number): number {
  const ms = Math.max(0, Math.round(elapsedMs));
  return maxAnswerSeconds > 0 ? Math.min(ms, maxAnswerSeconds * 1000) : ms;
}

/** m:ss for the optional timer. */
export function formatClock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`;
}

export type CustomKind =
  | { kind: 'ahead'; days: number }
  | { kind: 'forgotten'; days: number }
  | { kind: 'preview' }
  | { kind: 'search'; query: string; limit: number };

export const CUSTOM_SEARCH_LIMIT = 100;

/** Parses the route's `custom` param: 'ahead:3' | 'forgotten:7' | 'preview' | 'search:<q>' (optionally 'search:<q>|limit'). */
export function parseCustom(raw: string | undefined | null): CustomKind | null {
  if (!raw) return null;
  const s = raw.trim();
  if (s === 'preview') return { kind: 'preview' };
  const m = /^(ahead|forgotten):(\d{1,4})$/.exec(s);
  if (m) {
    const days = Number(m[2]);
    return days >= 1 ? { kind: m[1] as 'ahead' | 'forgotten', days: Math.min(days, 3650) } : null;
  }
  if (s.startsWith('search:')) {
    let query = s.slice('search:'.length);
    let limit = CUSTOM_SEARCH_LIMIT;
    const lm = /\|(\d{1,5})$/.exec(query);
    if (lm) { limit = Math.max(1, Number(lm[1])); query = query.slice(0, lm.index); }
    query = query.trim();
    return query ? { kind: 'search', query, limit } : null;
  }
  return null;
}

/** Inverse of parseCustom (for links). */
export function formatCustom(k: CustomKind): string {
  switch (k.kind) {
    case 'ahead': case 'forgotten': return `${k.kind}:${k.days}`;
    case 'preview': return 'preview';
    case 'search': return `search:${k.query}${k.limit !== CUSTOM_SEARCH_LIMIT ? `|${k.limit}` : ''}`;
  }
}

/** Splits 'Languages::Arabic' into parents and leaf for the top bar. */
export function splitDeckName(name: string): { parents: string[]; leaf: string } {
  const parts = name.split('::').map((p) => p.trim()).filter(Boolean);
  const leaf = parts.pop() ?? name;
  return { parents: parts, leaf };
}

/** Counts the one-by-one cloze spans the renderer marks on a side. */
export function countOneByOne(html: string): number {
  const seen = new Set<string>();
  for (const m of html.matchAll(/data-cloze-index="(\d+)"/g)) seen.add(m[1]!);
  return seen.size;
}

/** Back-side media to autoplay: Anki skips the question's audio on the answer unless asked to replay it. */
export function answerAudio(front: string[], back: string[], replayQuestion: boolean): string[] {
  if (replayQuestion) return back;
  let i = 0;
  while (i < front.length && i < back.length && front[i] === back[i]) i++;
  return back.slice(i);
}

/**
 * Field that shows on the question of this card: for cloze note types the first field with a cloze
 * for this ordinal; otherwise the first real field the question template references.
 */
export function frontFieldOrd(
  templates: Array<{ ord: number; qfmt: string }>, isCloze: boolean, cardOrd: number, fieldNames: string[], fields: string[],
): number {
  if (isCloze) {
    const re = new RegExp(`\\{\\{c${cardOrd + 1}::`);
    const i = fields.findIndex((f) => re.test(f));
    return i >= 0 ? i : 0;
  }
  const tmpl = templates.find((x) => x.ord === cardOrd) ?? templates[0];
  if (!tmpl) return 0;
  for (const m of tmpl.qfmt.matchAll(/\{\{([^#^/!{}][^{}]*)\}\}/g)) {
    const name = (m[1] ?? '').split(':').pop()!.trim();
    const i = fieldNames.indexOf(name);
    if (i >= 0) return i;
  }
  return 0;
}
