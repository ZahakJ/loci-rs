/**
 * Vertical bars: one series, stacked series (2px surface gap between segments, rounded data end
 * on the top segment only) or grouped series. Optional reference line (e.g. desired retention).
 * Hover / arrow keys show one tooltip listing every series for that category.
 */
import { useState } from 'react';
import { cx } from '@ui/design/primitives/layer';
import { ChartFrame, Legend, type TableData, type Tip, type Tone } from './ChartFrame';
import { bandScale, barPath, compactNumber, labelIndices, linearScale } from './scale';
import s from './charts.module.css';

export interface BarSeries { key: string; label: string; tone: Tone }
export interface BarDatum { label: string; values: number[]; /** extra tooltip rows */ extra?: Array<{ label: string; value: string }> }

export interface BarsProps {
  data: BarDatum[];
  series: BarSeries[];
  layout?: 'stacked' | 'grouped';
  height?: number;
  title: string;
  desc: string;
  /** formats values (tooltip, table, axis unless yTick given) */
  format?: (v: number) => string;
  yTick?: (v: number) => string;
  yMin?: number;
  yMax?: number;
  maxXLabels?: number;
  refLine?: { value: number; label: string };
  legend?: boolean;
  /** first column header of the hidden table */
  categoryHeader: string;
  /** highlight one category (e.g. today) */
  highlight?: number;
  className?: string;
}

const AXIS_W = 40, AXIS_H = 22, TOP = 8;

export function Bars({
  data, series, layout = 'stacked', height = 180, title, desc, format = (v) => v.toLocaleString(), yTick, yMin, yMax,
  maxXLabels = 8, refLine, legend = series.length > 1, categoryHeader, highlight, className,
}: BarsProps) {
  const [active, setActive] = useState<number | null>(null);
  const totals = data.map((d) => (layout === 'stacked' ? d.values.reduce((a, b) => a + b, 0) : Math.max(0, ...d.values)));
  const table: TableData = {
    caption: title,
    headers: [categoryHeader, ...series.map((x) => x.label)],
    rows: data.map((d) => [d.label, ...d.values.map(format)]),
  };

  return (
    <ChartFrame
      title={title}
      desc={desc}
      table={table}
      count={data.length}
      active={active}
      onActive={setActive}
      className={className}
      legend={legend ? <Legend items={series.map((x) => ({ label: x.label, tone: x.tone }))} /> : undefined}
    >
      {(width, ids) => {
        const plotW = Math.max(40, width - AXIS_W);
        const y = linearScale([...totals, refLine?.value ?? 0], [TOP + height, TOP], { min: yMin, max: yMax, integer: !yTick && !format(1.5).includes('.') });
        const band = bandScale(data.length, [AXIS_W, AXIS_W + plotW], { maxBar: layout === 'grouped' ? 24 * series.length + 4 : 24, gap: data.length > 60 ? 1 : 2 });
        const labels = new Set(labelIndices(data.length, Math.max(2, Math.min(maxXLabels, Math.floor(plotW / 56)))));
        const tick = yTick ?? ((v: number) => compactNumber(v));
        const base = y(Math.max(y.domain[0], 0));
        const tip: Tip | null = active !== null && data[active] ? {
          x: band.x(active) + band.band / 2,
          y: Math.max(0, y(totals[active] ?? 0) - 12) - 4,
          title: data[active]!.label,
          rows: [
            ...series.map((sr, k) => ({ label: sr.label, value: format(data[active]!.values[k] ?? 0), tone: sr.tone })),
            ...(data[active]!.extra ?? []),
          ],
        } : null;
        return (
          <>
            <svg width={width} height={TOP + height + AXIS_H} className={s.svg} role="img" aria-labelledby={ids.title} aria-describedby={ids.desc}>
              <title id={ids.title}>{title}</title>
              <desc id={ids.desc}>{desc}</desc>
              <g aria-hidden>
                {y.ticks.map((tv) => (
                  <g key={tv}>
                    <line className={s.grid} x1={AXIS_W} x2={AXIS_W + plotW} y1={Math.round(y(tv)) + 0.5} y2={Math.round(y(tv)) + 0.5} />
                    <text className={s.yTick} x={AXIS_W - 8} y={y(tv) + 4} textAnchor="end">{tick(tv)}</text>
                  </g>
                ))}
                {data.map((d, i) => {
                  const x0 = band.x(i) + band.offset;
                  const dim = active !== null && active !== i;
                  let acc = 0;
                  const parts = d.values.map((v, k) => {
                    if (layout === 'grouped') {
                      const sub = band.bar / series.length;
                      const w = Math.max(1, sub - (series.length > 1 ? 2 : 0));
                      return v > 0 ? <path key={k} className={cx(s.bar, s[`fill-${series[k]!.tone}`])} d={barPath(x0 + k * sub, w, base, y(v), Math.min(4, w / 2))} /> : null;
                    }
                    const yBottom = y(acc), yTop = y(acc + v);
                    acc += v;
                    if (v <= 0) return null;
                    const isTop = d.values.slice(k + 1).every((n) => n <= 0);
                    const gap = k > 0 ? 2 : 0;
                    const h = yBottom - yTop - gap;
                    if (h <= 0) return null;
                    return isTop
                      ? <path key={k} className={cx(s.bar, s[`fill-${series[k]!.tone}`])} d={barPath(x0, band.bar, yBottom - gap, yTop, Math.min(4, band.bar / 2))} />
                      : <rect key={k} className={cx(s.bar, s[`fill-${series[k]!.tone}`])} x={x0} y={yTop} width={band.bar} height={h} />;
                  });
                  return (
                    <g key={i} className={cx(s.col, dim && s.dim, highlight === i && s.highlight)}>
                      {parts}
                      {labels.has(i) && <text className={s.xTick} x={band.x(i) + band.band / 2} y={TOP + height + 16} textAnchor="middle">{d.label}</text>}
                      <rect className={s.hit} x={band.x(i)} y={TOP} width={band.band} height={height} onPointerEnter={() => setActive(i)} onPointerLeave={() => setActive(null)} />
                    </g>
                  );
                })}
                <line className={s.axis} x1={AXIS_W} x2={AXIS_W + plotW} y1={Math.round(base) + 0.5} y2={Math.round(base) + 0.5} />
                {refLine && (
                  <g className={s.ref}>
                    <line x1={AXIS_W} x2={AXIS_W + plotW} y1={y(refLine.value)} y2={y(refLine.value)} />
                    <text x={AXIS_W + plotW} y={y(refLine.value) - 6} textAnchor="end">{refLine.label}</text>
                  </g>
                )}
              </g>
            </svg>
            <TipLayer tip={tip} width={width} />
          </>
        );
      }}
    </ChartFrame>
  );
}

/** Tooltip drawn by the chart body (it knows positions); ChartFrame's own slot stays for others. */
export function TipLayer({ tip, width }: { tip: Tip | null; width: number }) {
  if (!tip) return null;
  const w = 184;
  return (
    <div className={s.tip} style={{ insetInlineStart: Math.max(0, Math.min(tip.x - w / 2, width - w)), insetBlockEnd: `calc(100% - ${Math.max(36, tip.y)}px)`, inlineSize: w }} aria-hidden>
      <p className={s.tipTitle}>{tip.title}</p>
      {tip.rows.map((r) => (
        <p key={r.label} className={s.tipRow}>
          {r.tone && <span className={cx(s.key, s[`tone-${r.tone}`], r.line && s.keyLine)} />}
          <span className={s.tipValue}>{r.value}</span>
          <span className={s.tipLabel}>{r.label}</span>
        </p>
      ))}
    </div>
  );
}
