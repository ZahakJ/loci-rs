import { useEffect, useRef, useState } from 'react';
import { cx } from './layer';
import s from './Heatmap.module.css';

export interface HeatmapDatum { date: string; count: number }
export interface HeatmapProps {
  data: HeatmapDatum[];
  /** ISO date of the last day; default = last datum or today */
  end?: string;
  /** accessible summary; a default is computed */
  label?: string;
  /** unit for cell titles, e.g. "reviews" (used when `countLabel` is not given) */
  unit?: string;
  /** "1 review" / "3 reviews": pluralised count for cell titles and the summary */
  countLabel?: (n: number) => string;
  /** accessible summary from the visible period's numbers (used when `label` is not given) */
  summaryLabel?: (v: { days: number; total: number; best: number }) => string;
  locale?: string;
  lessLabel?: string;
  moreLabel?: string;
  showLegend?: boolean;
  className?: string;
}

const CELL = 11, GAP = 3, STEP = CELL + GAP, TOP = 16, LEFT = 22;
const DAY_MS = 86_400_000;

const parse = (iso: string) => { const [y, m, d] = iso.split('-').map(Number); return Date.UTC(y!, (m ?? 1) - 1, d ?? 1); };
const iso = (t: number) => new Date(t).toISOString().slice(0, 10);

/** Quantile thresholds over non-zero days → 4 steps of the accent. */
export function heatLevels(counts: number[]): [number, number, number] {
  const nz = counts.filter((c) => c > 0).sort((a, b) => a - b);
  if (nz.length === 0) return [1, 2, 3];
  const q = (p: number) => nz[Math.min(nz.length - 1, Math.floor(p * nz.length))]!;
  return [q(0.25), q(0.5), q(0.75)];
}

/** 12-month calendar heatmap (Monday-first columns). Shows as many recent weeks as fit at ≥ 9px cells. */
export function Heatmap({ data, end, label, unit = 'reviews', countLabel, summaryLabel, locale, lessLabel = 'Less', moreLabel = 'More', showLegend = true, className }: HeatmapProps) {
  const wrap = useRef<HTMLDivElement>(null);
  const [weeks, setWeeks] = useState(53);
  useEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const ro = new ResizeObserver(([entry]) => {
      const w = entry?.contentRect.width ?? 0;
      // at 100% scale each week takes STEP px; allow scaling down to ~0.8
      const fit = Math.floor((w / 0.82 - LEFT) / STEP);
      setWeeks(Math.max(13, Math.min(53, fit)));
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const byDate = new Map(data.map((d) => [d.date, d.count]));
  const endT = parse(end ?? data[data.length - 1]?.date ?? iso(Date.now()));
  const endDow = (new Date(endT).getUTCDay() + 6) % 7; // Monday = 0
  const startT = endT - (endDow + (weeks - 1) * 7) * DAY_MS;
  const [t1, t2, t3] = heatLevels(data.map((d) => d.count));
  const level = (c: number) => (c <= 0 ? '' : c <= t1 ? s.l1 : c <= t2 ? s.l2 : c <= t3 ? s.l3 : s.l4);

  const fmtMonth = new Intl.DateTimeFormat(locale, { month: 'short', timeZone: 'UTC' });
  const fmtDay = new Intl.DateTimeFormat(locale, { weekday: 'short', timeZone: 'UTC' });
  const fmtFull = new Intl.DateTimeFormat(locale, { weekday: 'short', month: 'short', day: 'numeric', year: 'numeric', timeZone: 'UTC' });

  const cells = [];
  const months: Array<{ x: number; text: string }> = [];
  let lastMonth = -1;
  let studied = 0, total = 0, best = 0;
  for (let w = 0; w < weeks; w++) {
    for (let d = 0; d < 7; d++) {
      const t = startT + (w * 7 + d) * DAY_MS;
      if (t > endT) break;
      const key = iso(t);
      const c = byDate.get(key) ?? 0;
      if (c > 0) { studied++; total += c; best = Math.max(best, c); }
      const m = new Date(t).getUTCMonth();
      if (d === 0 && m !== lastMonth) {
        if (w < weeks - 2) months.push({ x: LEFT + w * STEP, text: fmtMonth.format(t) });
        lastMonth = m;
      }
      cells.push(
        <rect key={key} x={LEFT + w * STEP} y={TOP + d * STEP} width={CELL} height={CELL} rx={2.5} className={cx(s.cell, level(c), t === endT && s.today)}>
          <title>{`${fmtFull.format(t)}: ${countLabel ? countLabel(c) : `${c.toLocaleString(locale)} ${unit}`}`}</title>
        </rect>,
      );
    }
  }
  const width = LEFT + weeks * STEP - GAP;
  const height = TOP + 7 * STEP - GAP;
  const summary = label ?? summaryLabel?.({ days: studied, total, best }) ?? `${studied} days studied in this period, ${total.toLocaleString(locale)} ${unit} in total, most in one day ${best.toLocaleString(locale)}.`;
  const dayLabel = (d: number) => fmtDay.format(Date.UTC(2024, 0, 1 + d)); // 2024-01-01 was a Monday

  return (
    <div ref={wrap} className={cx(s.wrap, className)}>
      <svg className={s.svg} viewBox={`0 0 ${width} ${height}`} role="img" aria-label={summary} style={{ maxInlineSize: width * 1.25 }}>
        {months.map((m) => <text key={m.x} x={m.x} y={10} className={s.month}>{m.text}</text>)}
        {[0, 2, 4].map((d) => <text key={d} x={0} y={TOP + d * STEP + 9} className={s.day}>{dayLabel(d).slice(0, 3)}</text>)}
        {cells}
      </svg>
      {showLegend && (
        <div className={s.legend} aria-hidden>
          <span>{lessLabel}</span>
          {['', s.l1, s.l2, s.l3, s.l4].map((c, i) => (
            <svg key={i} width={CELL} height={CELL} viewBox={`0 0 ${CELL} ${CELL}`}><rect width={CELL} height={CELL} rx={2.5} className={cx(s.cell, c)} /></svg>
          ))}
          <span>{moreLabel}</span>
        </div>
      )}
    </div>
  );
}
