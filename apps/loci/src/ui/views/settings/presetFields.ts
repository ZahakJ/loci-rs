/**
 * Every DeckConfig field, grouped as in Anki's deck options, each with a one-line
 * explanation (research/anki-complaints.md C5). Labels/help are i18n keys resolved at render.
 * The "recommended" mark compares against STANDARD (DESIGN §7).
 */
import type { DeckConfig } from '@core/types';

export const FSRS6_DEFAULT = [
  0.212, 1.2931, 2.3065, 8.2956, 6.4133, 0.8334, 3.0194, 0.001, 1.8722, 0.1666, 0.796,
  1.4835, 0.0614, 0.2629, 1.6483, 0.6014, 1.8729, 0.5425, 0.0912, 0.0658, 0.1542,
];

/** The "Standard" preset (DESIGN §7). Kept in the UI so the recommended marks need no API call. */
export const STANDARD: DeckConfig = {
  scheduler: 'fsrs',
  newPerDay: 20, reviewsPerDay: 9999, minutesPerDay: 20,
  learnSteps: [1, 10], relearnSteps: [10],
  graduatingInterval: 1, easyInterval: 4,
  newOrder: 'due', newGather: 'deck', newSort: 'template-then-order',
  reviewOrder: 'due-then-random', newReviewMix: 'mix', interdayLearningMix: 'mix',
  initialEase: 2500, easyBonus: 1.3, intervalModifier: 1, hardMultiplier: 1.2, lapseMultiplier: 0,
  minimumLapseInterval: 1, maximumInterval: 36500,
  fsrsParams: FSRS6_DEFAULT, desiredRetention: 0.9, historicalRetention: 0.9,
  autoOptimise: true, optimiseEveryReviews: 400,
  leechThreshold: 8, leechAction: 'tag',
  burySiblingsNew: true, burySiblingsReview: true, burySiblingsInterday: false,
  autoplayAudio: true, replayQuestionAudio: true,
  maxAnswerSeconds: 60, showTimer: false, showIntervalsOnButtons: true,
};

/** Simple-layer learning presets (DESIGN §7: they differ only in retention and new cards). */
export const LEARNING_PRESETS = {
  relaxed: { desiredRetention: 0.85, newPerDay: 10 },
  standard: { desiredRetention: 0.9, newPerDay: 20 },
  intense: { desiredRetention: 0.93, newPerDay: 40 },
} as const;
export type LearningPreset = keyof typeof LEARNING_PRESETS;

export function detectLearningPreset(c: Pick<DeckConfig, 'desiredRetention' | 'newPerDay'>): LearningPreset | null {
  for (const [k, v] of Object.entries(LEARNING_PRESETS) as Array<[LearningPreset, { desiredRetention: number; newPerDay: number }]>) {
    if (Math.abs(c.desiredRetention - v.desiredRetention) < 0.0001 && c.newPerDay === v.newPerDay) return k;
  }
  return null;
}

export type GroupId = 'daily' | 'new' | 'lapses' | 'order' | 'fsrs' | 'burying' | 'audio' | 'timer' | 'advanced';
export const GROUPS: GroupId[] = ['daily', 'new', 'lapses', 'order', 'fsrs', 'burying', 'audio', 'timer', 'advanced'];

export type FieldKind =
  | { type: 'int'; min: number; max: number; unit?: string }
  | { type: 'float'; min: number; max: number; step: number; unit?: string }
  | { type: 'percent'; min: number; max: number; step: number }
  | { type: 'permille'; min: number; max: number }
  | { type: 'bool' }
  | { type: 'select'; options: string[] }
  | { type: 'steps' }
  | { type: 'params' };

export interface PresetField {
  key: keyof DeckConfig;
  group: GroupId;
  kind: FieldKind;
  /** only meaningful for this scheduler (shown, but marked) */
  only?: 'sm2' | 'fsrs';
}

export const PRESET_FIELDS: PresetField[] = [
  // Daily limits
  { key: 'newPerDay', group: 'daily', kind: { type: 'int', min: 0, max: 9999, unit: 'cards' } },
  { key: 'reviewsPerDay', group: 'daily', kind: { type: 'int', min: 0, max: 99999, unit: 'cards' } },
  { key: 'minutesPerDay', group: 'daily', kind: { type: 'int', min: 0, max: 600, unit: 'min' } },
  // New cards
  { key: 'learnSteps', group: 'new', kind: { type: 'steps' } },
  { key: 'graduatingInterval', group: 'new', kind: { type: 'int', min: 1, max: 365, unit: 'days' }, only: 'sm2' },
  { key: 'easyInterval', group: 'new', kind: { type: 'int', min: 1, max: 365, unit: 'days' }, only: 'sm2' },
  { key: 'newOrder', group: 'new', kind: { type: 'select', options: ['due', 'random'] } },
  // Lapses
  { key: 'relearnSteps', group: 'lapses', kind: { type: 'steps' } },
  { key: 'minimumLapseInterval', group: 'lapses', kind: { type: 'int', min: 1, max: 365, unit: 'days' }, only: 'sm2' },
  { key: 'lapseMultiplier', group: 'lapses', kind: { type: 'percent', min: 0, max: 1, step: 0.05 }, only: 'sm2' },
  { key: 'leechThreshold', group: 'lapses', kind: { type: 'int', min: 1, max: 99, unit: 'lapses' } },
  { key: 'leechAction', group: 'lapses', kind: { type: 'select', options: ['tag', 'suspend'] } },
  // Display order
  { key: 'newGather', group: 'order', kind: { type: 'select', options: ['deck', 'deck-random-notes', 'ascending', 'descending', 'random-notes', 'random-cards'] } },
  { key: 'newSort', group: 'order', kind: { type: 'select', options: ['template-then-order', 'order', 'template-then-random', 'random-note', 'random-card'] } },
  { key: 'newReviewMix', group: 'order', kind: { type: 'select', options: ['mix', 'review-first', 'new-first'] } },
  { key: 'interdayLearningMix', group: 'order', kind: { type: 'select', options: ['mix', 'review-first', 'new-first'] } },
  { key: 'reviewOrder', group: 'order', kind: { type: 'select', options: ['due-then-random', 'due-then-deck', 'deck-then-due', 'ascending-intervals', 'descending-intervals', 'ascending-ease', 'descending-ease', 'relative-overdue', 'random', 'added', 'reverse-added'] } },
  // FSRS
  { key: 'scheduler', group: 'fsrs', kind: { type: 'select', options: ['fsrs', 'sm2'] } },
  { key: 'desiredRetention', group: 'fsrs', kind: { type: 'percent', min: 0.7, max: 0.99, step: 0.01 }, only: 'fsrs' },
  { key: 'autoOptimise', group: 'fsrs', kind: { type: 'bool' }, only: 'fsrs' },
  { key: 'optimiseEveryReviews', group: 'fsrs', kind: { type: 'int', min: 100, max: 100000, unit: 'reviews' }, only: 'fsrs' },
  { key: 'historicalRetention', group: 'fsrs', kind: { type: 'percent', min: 0.5, max: 0.99, step: 0.01 }, only: 'fsrs' },
  { key: 'fsrsParams', group: 'fsrs', kind: { type: 'params' }, only: 'fsrs' },
  // Burying
  { key: 'burySiblingsNew', group: 'burying', kind: { type: 'bool' } },
  { key: 'burySiblingsReview', group: 'burying', kind: { type: 'bool' } },
  { key: 'burySiblingsInterday', group: 'burying', kind: { type: 'bool' } },
  // Audio
  { key: 'autoplayAudio', group: 'audio', kind: { type: 'bool' } },
  { key: 'replayQuestionAudio', group: 'audio', kind: { type: 'bool' } },
  // Timer
  { key: 'maxAnswerSeconds', group: 'timer', kind: { type: 'int', min: 5, max: 3600, unit: 'seconds' } },
  { key: 'showTimer', group: 'timer', kind: { type: 'bool' } },
  // Advanced
  { key: 'maximumInterval', group: 'advanced', kind: { type: 'int', min: 1, max: 36500, unit: 'days' } },
  { key: 'showIntervalsOnButtons', group: 'advanced', kind: { type: 'bool' } },
  { key: 'initialEase', group: 'advanced', kind: { type: 'permille', min: 1310, max: 5000 }, only: 'sm2' },
  { key: 'easyBonus', group: 'advanced', kind: { type: 'float', min: 1, max: 5, step: 0.05, unit: '×' }, only: 'sm2' },
  { key: 'intervalModifier', group: 'advanced', kind: { type: 'float', min: 0.5, max: 2, step: 0.05, unit: '×' }, only: 'sm2' },
  { key: 'hardMultiplier', group: 'advanced', kind: { type: 'float', min: 0.5, max: 3, step: 0.05, unit: '×' }, only: 'sm2' },
];

export function isRecommended(key: keyof DeckConfig, value: unknown): boolean {
  const rec = STANDARD[key];
  if (Array.isArray(rec) && Array.isArray(value)) return rec.length === value.length && rec.every((v, i) => Math.abs(v - (value[i] as number)) < 1e-9);
  if (typeof rec === 'number' && typeof value === 'number') return Math.abs(rec - value) < 1e-9;
  return rec === value;
}

/** Validates an FSRS parameter list typed or pasted by the user (17, 19 or 21 numbers). */
export function parseParams(text: string): number[] | null {
  const parts = text.split(/[\s,;]+/).filter(Boolean);
  if (![17, 19, 21].includes(parts.length)) return null;
  const nums = parts.map(Number);
  return nums.every((n) => Number.isFinite(n)) ? nums : null;
}

/** Relative review workload of a desired retention against a baseline (FSRS forgetting curve). */
export function workloadRatio(r: number, base = 0.85): number {
  const f = (x: number) => 1 / (Math.pow(x, -2) - 1);
  return f(r) / f(base);
}
