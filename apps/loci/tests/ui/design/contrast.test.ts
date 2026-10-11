import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { contrast, evaluate, oklchToHex, parseColor, tokenBlock } from '../../../scripts/contrast.ts';

describe('contrast: OKLCH to sRGB', () => {
  it('converts known colours', () => {
    expect(oklchToHex('oklch(62.8% 0.2577 29.23)')).toBe('#ff0000');
    expect(oklchToHex('oklch(100% 0 0)')).toBe('#ffffff');
    expect(oklchToHex('oklch(0% 0 0)')).toBe('#000000');
    expect(oklchToHex('oklch(0.452 0.313 264.05)')).toBe('#0000ff');
  });

  it('round-trips hex through OKLab', () => {
    expect(oklchToHex('#fff')).toBe('#ffffff');
    expect(oklchToHex('#336699')).toBe('#336699');
  });

  it('computes WCAG ratios', () => {
    expect(contrast(parseColor('#fff'), parseColor('#000'))).toBeCloseTo(21, 5);
    expect(contrast(parseColor('#777'), parseColor('#fff'))).toBeCloseTo(4.48, 2);
  });
});

describe('contrast: the tokens', () => {
  const css = readFileSync(new URL('../../../src/ui/design/tokens.css', import.meta.url), 'utf8');
  const light = tokenBlock(css, ':root');
  const dark = { ...light, ...tokenBlock(css, ":root[data-theme='dark']") };
  const pill = { text: 0.68, bg: 0.13 };

  it('every required pair passes in both themes', () => {
    const failed = [...evaluate('light', light, pill), ...evaluate('dark', dark, pill)].filter((r) => r.required && !r.pass);
    expect(failed.map((r) => `${r.theme} ${r.pair} ${r.ratio.toFixed(2)}`)).toEqual([]);
  });

  it('the OS-dark block repeats the explicit dark block', () => {
    expect(tokenBlock(css, ":root:not([data-theme='light'])")).toEqual(tokenBlock(css, ":root[data-theme='dark']"));
  });
});
