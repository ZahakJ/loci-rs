import { describe, expect, it } from 'vitest';
import { bandScale, barPath, labelIndices, linearScale, niceStep, niceTicks, niceTicksFixed } from '@ui/charts/scale';

describe('chart scales', () => {
  it('picks nice steps', () => {
    expect(niceStep(100, 4)).toBe(25);
    expect(niceStep(7, 4)).toBe(2);
    expect(niceStep(0.9, 3)).toBe(0.5);
    expect(niceStep(1300, 4)).toBe(500);
  });
  it('makes 3–5 ticks covering the data from zero', () => {
    for (const max of [1, 7, 13, 97, 412, 1830, 13240, 0.87]) {
      const t = niceTicks(0, max);
      expect(t.length).toBeGreaterThanOrEqual(3);
      expect(t.length).toBeLessThanOrEqual(5);
      expect(t[0]).toBe(0);
      expect(t.at(-1)!).toBeGreaterThanOrEqual(max);
    }
    expect(niceTicks(0, 3, { integer: true }).every(Number.isInteger)).toBe(true);
    expect(niceTicks(0, 0).at(-1)).toBe(1);
  });
  it('maps and inverts a linear scale', () => {
    const y = linearScale([0, 40, 90], [200, 0]);
    expect(y.domain).toEqual([0, 100]);
    expect(y(50)).toBe(100);
    expect(y.invert(100)).toBe(50);
    const pct = linearScale([], [100, 0], { min: 0.5, max: 1 });
    expect(pct.ticks[0]).toBe(0.5);
    expect(pct.ticks.at(-1)).toBe(1);
  });
  it('fixed ticks stay inside the domain', () => {
    expect(niceTicksFixed(0, 1, 5)).toEqual([0, 0.25, 0.5, 0.75, 1]);
  });
  it('caps bar thickness and keeps a gap', () => {
    const b = bandScale(10, [0, 1000]);
    expect(b.band).toBe(100);
    expect(b.bar).toBe(24);
    expect(b.offset).toBe(38);
    const dense = bandScale(90, [0, 270]);
    expect(dense.bar).toBe(1);
    expect(bandScale(4, [0, 100]).index(99)).toBe(3);
    expect(bandScale(4, [0, 100]).index(-5)).toBe(0);
  });
  it('draws a bar with a rounded data end and nothing when empty', () => {
    expect(barPath(0, 10, 100, 100)).toBe('');
    expect(barPath(0, 10, 100, 50)).toMatch(/^M0,100V54Q0,50 4,50H6Q10,50 10,54V100Z$/);
  });
  it('thins axis labels', () => {
    expect(labelIndices(5, 10)).toEqual([0, 1, 2, 3, 4]);
    expect(labelIndices(24, 6)).toEqual([0, 4, 8, 12, 16, 20]);
  });
});
