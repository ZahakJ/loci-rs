/**
 * Shared scale and axis helpers for the hand-drawn SVG charts (pure; tested).
 * Linear scales snap to "nice" 1-2-2.5-5 steps and produce 3–5 ticks; band scales place
 * bars with a capped thickness so the leftover band is air (dataviz mark spec).
 */

export interface LinearScale {
  (v: number): number;
  domain: [number, number];
  range: [number, number];
  ticks: number[];
  invert: (px: number) => number;
}

/** A "nice" step for a span and a target number of intervals: 1, 2, 2.5 or 5 × 10^k. */
export function niceStep(span: number, intervals: number): number {
  if (!(span > 0) || !(intervals > 0)) return 1;
  const raw = span / intervals;
  const pow = Math.pow(10, Math.floor(Math.log10(raw)));
  const f = raw / pow;
  const nice = f <= 1 ? 1 : f <= 2 ? 2 : f <= 2.5 ? 2.5 : f <= 5 ? 5 : 10;
  return nice * pow;
}

/**
 * Ticks from 0 (or min) to a nice max covering `max`, 3–5 ticks (including the baseline).
 * `integer` forbids fractional steps (counts).
 */
export function niceTicks(min: number, max: number, opts: { maxTicks?: number; integer?: boolean } = {}): number[] {
  const maxTicks = Math.max(3, Math.min(opts.maxTicks ?? 5, 6));
  if (!Number.isFinite(min) || !Number.isFinite(max)) return [0, 1];
  if (max <= min) max = min + (opts.integer ? Math.max(1, Math.abs(min)) : Math.abs(min) || 1);
  let best: number[] = [];
  for (let intervals = maxTicks - 1; intervals >= 2; intervals--) {
    let step = niceStep(max - min, intervals);
    if (opts.integer) step = Math.max(1, Math.ceil(step));
    const lo = Math.floor(min / step) * step;
    const hi = Math.ceil(max / step) * step;
    const n = Math.round((hi - lo) / step);
    const ticks = Array.from({ length: n + 1 }, (_, i) => roundTo(lo + i * step, step));
    if (ticks.length >= 3 && ticks.length <= maxTicks) { best = ticks; break; }
    if (!best.length || Math.abs(ticks.length - 4) < Math.abs(best.length - 4)) best = ticks;
  }
  return best;
}

function roundTo(v: number, step: number): number {
  const digits = Math.max(0, -Math.floor(Math.log10(step)) + 2);
  return Number(v.toFixed(Math.min(10, digits)));
}

/** Linear scale over a nice domain (baseline 0 by default). */
export function linearScale(values: number[], range: [number, number], opts: { min?: number; max?: number; zero?: boolean; maxTicks?: number; integer?: boolean } = {}): LinearScale {
  const finite = values.filter(Number.isFinite);
  let lo = opts.min ?? Math.min(...finite, opts.zero === false ? Infinity : 0);
  let hi = opts.max ?? Math.max(...finite, lo);
  if (!Number.isFinite(lo)) lo = 0;
  if (!Number.isFinite(hi)) hi = lo + 1;
  const ticks = opts.min !== undefined && opts.max !== undefined
    ? niceTicksFixed(opts.min, opts.max, opts.maxTicks ?? 5)
    : niceTicks(lo, hi, opts);
  const d0 = opts.min ?? ticks[0]!, d1 = opts.max ?? ticks[ticks.length - 1]!;
  const span = d1 - d0 || 1;
  const [r0, r1] = range;
  const f = ((v: number) => r0 + ((v - d0) / span) * (r1 - r0)) as LinearScale;
  f.domain = [d0, d1];
  f.range = range;
  f.ticks = ticks;
  f.invert = (px: number) => d0 + ((px - r0) / (r1 - r0 || 1)) * span;
  return f;
}

/** Ticks inside a fixed domain (e.g. 0–100%). */
export function niceTicksFixed(min: number, max: number, maxTicks = 5): number[] {
  let step = niceStep(max - min, Math.max(2, maxTicks - 1));
  const onGrid = (v: number, st: number) => Math.abs(v / st - Math.round(v / st)) < 1e-9;
  for (let k = Math.max(2, maxTicks - 1); k >= 2; k--) {
    const st = niceStep(max - min, k);
    if (onGrid(min, st) && onGrid(max, st)) { step = st; break; }
  }
  const out: number[] = [];
  for (let v = Math.ceil(min / step) * step; v <= max + step * 1e-9; v += step) out.push(roundTo(v, step));
  return out;
}

export interface BandScale {
  /** start of the band for index i */
  x: (i: number) => number;
  /** band width */
  band: number;
  /** bar thickness (≤ maxBar) and its offset inside the band */
  bar: number;
  offset: number;
  /** index under a pixel position (clamped) */
  index: (px: number) => number;
}

/** Evenly spaced bands with bars capped at `maxBar` px and at least `gap` px between bars. */
export function bandScale(count: number, range: [number, number], opts: { maxBar?: number; gap?: number } = {}): BandScale {
  const [r0, r1] = range;
  const n = Math.max(1, count);
  const band = (r1 - r0) / n;
  const gap = opts.gap ?? 2;
  const bar = Math.max(1, Math.min(opts.maxBar ?? 24, band - gap));
  const offset = (band - bar) / 2;
  return {
    x: (i) => r0 + i * band,
    band, bar, offset,
    index: (px) => Math.max(0, Math.min(n - 1, Math.floor((px - r0) / band))),
  };
}

/** Compact number for axis labels: 950, 1.2k, 12k, 1.5M. */
export function compactNumber(v: number, locale?: string): string {
  return new Intl.NumberFormat(locale, { notation: Math.abs(v) >= 10_000 ? 'compact' : 'standard', maximumFractionDigits: Math.abs(v) >= 10_000 ? 1 : 2 }).format(v);
}

/**
 * Path for a bar with a rounded data end (radius r) and a square baseline, growing up from
 * `y0` to `y1` (y1 < y0). Degenerates to a rectangle when the bar is shorter than r.
 */
export function barPath(x: number, w: number, y0: number, y1: number, r = 4): string {
  const h = y0 - y1;
  if (h <= 0) return '';
  const rr = Math.min(r, w / 2, h);
  return `M${x},${y0}V${y1 + rr}Q${x},${y1} ${x + rr},${y1}H${x + w - rr}Q${x + w},${y1} ${x + w},${y1 + rr}V${y0}Z`;
}

/** Horizontal bar growing right from x0 to x1 with a rounded end. */
export function hbarPath(y: number, h: number, x0: number, x1: number, r = 4): string {
  const w = x1 - x0;
  if (w <= 0) return '';
  const rr = Math.min(r, h / 2, w);
  return `M${x0},${y}H${x1 - rr}Q${x1},${y} ${x1},${y + rr}V${y + h - rr}Q${x1},${y + h} ${x1 - rr},${y + h}H${x0}Z`;
}

/** Evenly spread label indices so at most `max` category labels show on an axis. */
export function labelIndices(count: number, max: number): number[] {
  if (count <= max) return Array.from({ length: count }, (_, i) => i);
  const step = Math.ceil(count / max);
  const out: number[] = [];
  for (let i = 0; i < count; i += step) out.push(i);
  return out;
}
