import { describe, expect, it } from 'vitest';
import { formatBytes, formatMinutes, formatSteps, parseSteps } from '@ui/hooks/format';
import { DECKS } from '@ui/dev/mockData';
import { GROUPS, PRESET_FIELDS, STANDARD, detectLearningPreset, isRecommended, parseParams, workloadRatio } from '@ui/views/settings/presetFields';
import en from '@core/i18n/en.json';

describe('format', () => {
  it('formats minutes', () => {
    expect(formatMinutes(0)).toBe('less than a minute');
    expect(formatMinutes(14)).toBe('14 min');
    expect(formatMinutes(60)).toBe('1 hour');
    expect(formatMinutes(130)).toBe('2 h 10 min');
  });
  it('formats bytes', () => {
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(9_400_000)).toBe('9 MB');
    expect(formatBytes(1536)).toBe('1.5 KB');
  });
  it('parses and formats learning steps', () => {
    expect(parseSteps('1m 10m')).toEqual([1, 10]);
    expect(parseSteps('30s, 1h 1d')).toEqual([0.5, 60, 1440]);
    expect(parseSteps('10')).toEqual([10]);
    expect(parseSteps('1x')).toBeNull();
    expect(parseSteps('0m')).toBeNull();
    expect(parseSteps('')).toEqual([]);
    expect(formatSteps([1, 10, 60, 1440, 0.5])).toBe('1m 10m 1h 1d 30s');
  });
});

describe('preset fields', () => {
  it('covers every DeckConfig field exactly once', () => {
    const keys = PRESET_FIELDS.map((f) => f.key).sort();
    expect(keys).toEqual(Object.keys(STANDARD).sort());
    for (const f of PRESET_FIELDS) expect(GROUPS).toContain(f.group);
  });
  it('has a label and an explanation for every field and option', () => {
    const dict = flat(en);
    for (const f of PRESET_FIELDS) {
      expect(dict[`preset.f.${f.key}`], f.key).toBeTruthy();
      expect(dict[`preset.h.${f.key}`], f.key).toBeTruthy();
      if (f.kind.type === 'select') for (const o of f.kind.options) expect(dict[`preset.opt.${f.key}.${o}`], `${f.key}.${o}`).toBeTruthy();
    }
    for (const g of GROUPS) expect(dict[`preset.group.${g}`]).toBeTruthy();
  });
  it('marks recommended values', () => {
    expect(isRecommended('learnSteps', [1, 10])).toBe(true);
    expect(isRecommended('learnSteps', [1, 10, 60])).toBe(false);
    expect(isRecommended('desiredRetention', 0.9)).toBe(true);
    expect(isRecommended('leechAction', 'suspend')).toBe(false);
  });
  it('detects learning presets', () => {
    expect(detectLearningPreset({ desiredRetention: 0.85, newPerDay: 10 })).toBe('relaxed');
    expect(detectLearningPreset({ desiredRetention: 0.9, newPerDay: 20 })).toBe('standard');
    expect(detectLearningPreset({ desiredRetention: 0.93, newPerDay: 40 })).toBe('intense');
    expect(detectLearningPreset({ desiredRetention: 0.9, newPerDay: 25 })).toBeNull();
  });
  it('validates pasted FSRS parameters', () => {
    expect(parseParams(STANDARD.fsrsParams.join(', '))).toHaveLength(21);
    expect(parseParams(STANDARD.fsrsParams.slice(0, 17).join(' '))).toHaveLength(17);
    expect(parseParams('1, 2, 3')).toBeNull();
    expect(parseParams(Array(21).fill('x').join(','))).toBeNull();
  });
  it('workload grows with retention', () => {
    expect(workloadRatio(0.85)).toBeCloseTo(1);
    expect(workloadRatio(0.9)).toBeGreaterThan(1.5);
    expect(workloadRatio(0.95)).toBeGreaterThan(workloadRatio(0.9));
  });
  it('mock data keeps six decks', () => expect(DECKS).toHaveLength(6));
});

function flat(obj: unknown, prefix = '', out: Record<string, string> = {}): Record<string, string> {
  for (const [k, v] of Object.entries(obj as Record<string, unknown>)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (typeof v === 'string') out[key] = v; else if (v && typeof v === 'object') flat(v, key, out);
  }
  return out;
}
