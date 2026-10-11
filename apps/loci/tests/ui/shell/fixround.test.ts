/** Fix-round helpers: presets by card count, toast stacking, touch detection, bottom bars, copy. */
import { beforeAll, describe, expect, it } from 'vitest';
import type { DeckConfigPreset, DeckTreeNode, WorkloadForecast } from '@core/types';
import { STANDARD } from '@ui/views/settings/presetFields';
import { cardsPerPreset, decksUsing, primaryPreset } from '@ui/hooks/presets';
import { isTouchOnly } from '@ui/design/primitives/pointer';
import { barSpace } from '@ui/design/primitives/bottomBar';
import { localDateStamp } from '@ui/hooks/format';
import { budgetLine } from '@ui/views/home/budget';
import { dailyTime, forecastSentence } from '@ui/views/settings/forecast';
import { lossChange, unsavedEdits } from '@ui/views/settings/presetEdit';
import { importButtonLabel, isLociBackupFile, uniqueWarnings, type PreviewCounts } from '@ui/views/import/importText';

const node = (id: number, name: string, configId: number, totalCards: number, children: DeckTreeNode[] = [], level = 1): DeckTreeNode => ({
  deck: { id, name, configId, description: '', collapsed: false, today: { day: 0, new: 0, rev: 0, lrn: 0, timeMs: 0 }, mtime: 0, usn: 0 },
  children, level, newCount: 0, learnCount: 0, reviewCount: 0, estimatedMinutes: 0, totalCards,
});
const preset = (id: number, name: string): DeckConfigPreset => ({ id, name, config: STANDARD, mtime: 0, usn: 0 });

// Default (preset 1) holds 2 cards; Languages (5) has 10 own + Arabic (5) 30; Medicine (7) 25.
const tree = node(0, '', 1, 67, [
  node(1, 'Default', 1, 2),
  node(2, 'Languages', 5, 40, [node(3, 'Languages::Arabic', 5, 30, [], 2)]),
  node(4, 'Medicine', 7, 25),
], 0);
const presets = [preset(1, 'Standard'), preset(5, 'Languages'), preset(7, 'Medicine'), preset(9, 'Unused')];

describe('presets by card count', () => {
  it('counts each deck once (parents include their children)', () => {
    expect(Object.fromEntries(cardsPerPreset(tree))).toEqual({ 1: 2, 5: 40, 7: 25 });
  });
  it('defaults to the preset most cards use, not id 1', () => {
    expect(primaryPreset(tree, presets)?.id).toBe(5);
    expect(primaryPreset(undefined, presets)).toBeUndefined();
    expect(primaryPreset(tree, [])).toBeUndefined();
  });
  it('breaks ties by decks, then the lowest id', () => {
    const flat = node(0, '', 1, 0, [node(1, 'A', 3, 0), node(2, 'B', 2, 0), node(3, 'C', 2, 0)], 0);
    expect(primaryPreset(flat, [preset(2, 'two'), preset(3, 'three')])?.id).toBe(2);
    expect(primaryPreset(node(0, '', 1, 0, [], 0), [preset(4, 'x'), preset(3, 'y')])?.id).toBe(3);
  });
  it('lists the decks using a preset', () => {
    expect(decksUsing(tree, 5)).toEqual(['Languages', 'Languages::Arabic']);
    expect(decksUsing(tree, 9)).toEqual([]);
  });
});

describe('toasts', () => {
  beforeAll(() => { (globalThis as { window?: unknown }).window ??= globalThis; });
  it('keep at most two, newest last in the store (shown on top)', async () => {
    const { useToasts, MAX_TOASTS, toast } = await import('@ui/design/primitives/Toast');
    expect(MAX_TOASTS).toBe(2);
    toast({ message: 'one', duration: Infinity });
    toast({ message: 'two', duration: Infinity });
    toast({ message: 'three', duration: Infinity });
    expect(useToasts.getState().toasts.map((x) => x.message)).toEqual(['two', 'three']);
    toast({ message: 'three again', id: useToasts.getState().toasts[1]!.id, duration: Infinity });
    expect(useToasts.getState().toasts.map((x) => x.message)).toEqual(['two', 'three again']);
  });
});

describe('touch-only detection', () => {
  const m = (on: string[]) => (q: string) => on.includes(q);
  it('is touch when the pointer is coarse and cannot hover', () => {
    expect(isTouchOnly(m(['(pointer: coarse)', '(hover: none)']))).toBe(true);
  });
  it('is touch when coarse with no fine pointer anywhere', () => {
    expect(isTouchOnly(m(['(pointer: coarse)']))).toBe(true);
  });
  it('is not touch with a mouse, or a touch laptop with a trackpad', () => {
    expect(isTouchOnly(m(['(pointer: fine)', '(any-pointer: fine)']))).toBe(false);
    expect(isTouchOnly(m(['(pointer: coarse)', '(any-pointer: fine)']))).toBe(false);
  });
});

describe('bottom bars', () => {
  it('reserve the space from their top edge to the viewport bottom', () => {
    expect(barSpace({ top: 771, height: 129 }, 900)).toBe(129);
    expect(barSpace({ top: 760, height: 64 }, 844)).toBe(84); // sticky bar 20px above the bottom
  });
  it('reserve nothing when hidden or scrolled away', () => {
    expect(barSpace({ top: 0, height: 0 }, 900)).toBe(0);
    expect(barSpace({ top: 950, height: 60 }, 900)).toBe(0);
    expect(barSpace({ top: 300, height: 60 }, 900)).toBe(0);
  });
});

describe('copy', () => {
  it('dates files in local time', () => {
    expect(localDateStamp(new Date(2026, 0, 5, 0, 30))).toBe('2026-01-05');
    expect(localDateStamp(new Date(2026, 11, 31, 23, 59))).toBe('2026-12-31');
  });
  it('shows progress against the daily budget once studied', () => {
    expect(budgetLine(0, 20)).toBeNull();
    expect(budgetLine(undefined, 20)).toBeNull();
    expect(budgetLine(360, 0)).toBeNull();
    expect(budgetLine(20, 20)).toBe('Under a minute of 20 min today');
    expect(budgetLine(6 * 60 + 10, 20)).toBe('6 of 20 min today');
  });
  it('never says "about 0 min" in the workload forecast', () => {
    const f = (reviews: number, minutes: number): WorkloadForecast => ({ reviewsPerDay: Array(90).fill(reviews), minutesPerDay: Array(90).fill(minutes), sentence: 'core' });
    expect(dailyTime(0.2)).toBe('under a minute');
    expect(dailyTime(6.4)).toBe('about 6 min');
    expect(forecastSentence(f(3, 0.3))).toBe('About 3 reviews a day over the next month (under a minute), settling near 3 a day after that.');
    expect(forecastSentence(f(41, 6))).toContain('(about 6 min)');
    expect(forecastSentence(f(0, 0))).toMatch(/^Almost nothing/);
    expect(forecastSentence({ reviewsPerDay: [], minutesPerDay: [], sentence: 'core' })).toBe('core');
  });
  it('words the optimise change with enough decimals', () => {
    expect(lossChange(0.3114, 0.2903)).toBe('0.31 → 0.29');
    expect(lossChange(0.3114, 0.3101)).toBe('0.311 → 0.310');
    expect(lossChange(0.3, 0.3)).toBe('0.30 → 0.30');
  });
  it('finds unsaved edits against the stored preset', () => {
    expect(unsavedEdits(STANDARD, { ...STANDARD, leechThreshold: 9, learnSteps: [1, 10] })).toEqual({ leechThreshold: 9 });
    expect(unsavedEdits(STANDARD, { ...STANDARD, learnSteps: [2] })).toEqual({ learnSteps: [2] });
  });
});

describe('import', () => {
  // older collections did not send notesExisting/notesNew, so the base preview leaves them out
  const base = { layout: 'legacy2' as const, isCollectionPackage: false, notes: 40, cards: 50, decks: [], notetypes: [], reviews: 0, media: 0, hasScheduling: true, fsrsCards: 0, sourceCrt: 0, warnings: [] } as unknown as PreviewCounts;
  it('labels the button by what is new', () => {
    expect(importButtonLabel(base, 'merge')).toBe('Import 40 notes');
    expect(importButtonLabel({ ...base, notesExisting: 28, notesNew: 12 }, 'merge')).toBe('Import 12 new notes');
    expect(importButtonLabel({ ...base, notesExisting: 39, notesNew: 1 }, 'merge')).toBe('Import 1 new note');
    expect(importButtonLabel({ ...base, notesExisting: 40, notesNew: 0 }, 'merge')).toBeNull();
    expect(importButtonLabel({ ...base, notesExisting: 40, notesNew: 0 }, 'replace')).toBe('Import 40 notes');
  });
  it('says each warning once', () => {
    const f1 = 'Filtered deck "A" was not imported; 3 card(s) went back to their home decks.';
    const f2 = '3 card(s) from filtered decks went back to their home decks.';
    expect(uniqueWarnings([f1, f1, 'x', f2])).toEqual([f1, 'x']);
    expect(uniqueWarnings([f2, 'y'], [f1])).toEqual(['y']);
  });
  it('recognises a Loci full backup', () => {
    expect(isLociBackupFile('loci-backup-2026-10-07.zip')).toBe(true);
    expect(isLociBackupFile('deck.apkg')).toBe(false);
  });
});
