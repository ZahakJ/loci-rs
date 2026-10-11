/** Pure helpers for the preset editor (tested). */
import type { DeckConfig } from '@core/types';

/** Fields the user changed in the form but has not saved: draft vs the stored config. */
export function unsavedEdits(saved: DeckConfig, draft: DeckConfig): Partial<DeckConfig> {
  const out: Partial<DeckConfig> = {};
  for (const k of Object.keys(draft) as Array<keyof DeckConfig>) {
    if (JSON.stringify(draft[k]) !== JSON.stringify(saved[k])) (out as Record<string, unknown>)[k] = draft[k];
  }
  return out;
}

/** 'Log loss 0.31 → 0.29' with enough decimals that the two numbers differ when they do. */
export function lossChange(before: number, after: number): string {
  let dp = 2;
  while (dp < 5 && before.toFixed(dp) === after.toFixed(dp) && before !== after) dp++;
  return `${before.toFixed(dp)} → ${after.toFixed(dp)}`;
}
