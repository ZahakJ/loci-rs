/**
 * One horizontal stacked bar (part-to-whole, never a pie): segments separated by a 2px surface
 * gap, rounded only at the ends, with a legend carrying each value and share.
 */
import { useState } from 'react';
import { cx } from '@ui/design/primitives/layer';
import { TipLayer } from './Bars';
import { ChartFrame, Legend, type TableData, type Tone } from './ChartFrame';
import s from './charts.module.css';

export interface StackedSegment { key: string; label: string; value: number; tone: Tone }

export interface StackedBarProps {
  segments: StackedSegment[];
  title: string;
  desc: string;
  format?: (v: number) => string;
  percent?: (share: number) => string;
  categoryHeader: string;
  valueHeader: string;
  height?: number;
  className?: string;
}

export function StackedBar({ segments, title, desc, format = (v) => v.toLocaleString(), percent = (x) => `${Math.round(x * 100)}%`, categoryHeader, valueHeader, height = 20, className }: StackedBarProps) {
  const [active, setActive] = useState<number | null>(null);
  const total = segments.reduce((a, x) => a + Math.max(0, x.value), 0) || 1;
  const visible = segments.filter((x) => x.value > 0);
  const table: TableData = { caption: title, headers: [categoryHeader, valueHeader], rows: segments.map((x) => [x.label, `${format(x.value)} (${percent(x.value / total)})`]) };
  return (
    <ChartFrame title={title} desc={desc} table={table} count={visible.length} active={active} onActive={setActive} className={className}
      legend={<Legend className={s.legendGrid} items={segments.map((x) => ({ label: x.label, tone: x.tone, value: `${format(x.value)} · ${percent(x.value / total)}` }))} />}>
      {(width, ids) => {
        const gap = 2;
        const usable = width - gap * Math.max(0, visible.length - 1);
        let x = 0;
        const rects = visible.map((seg, i) => {
          const w = Math.max(2, (seg.value / total) * usable);
          const r = { seg, x, w, i };
          x += w + gap;
          return r;
        });
        const a = active !== null ? rects[active] : undefined;
        const clip = `${ids.title.replace(/[^a-zA-Z0-9-]/g, '')}-clip`;
        return (
          <>
            <svg width={width} height={height + 4} className={s.svg} role="img" aria-labelledby={ids.title} aria-describedby={ids.desc}>
              <title id={ids.title}>{title}</title>
              <desc id={ids.desc}>{desc}</desc>
              <defs>
                <clipPath id={clip}><rect x={0} y={2} width={Math.max(0, x - gap)} height={height} rx={6} /></clipPath>
              </defs>
              <g clipPath={`url(#${clip})`} aria-hidden>
                {rects.map((r) => (
                  <rect key={r.seg.key} className={cx(s.bar, s[`fill-${r.seg.tone}`], active !== null && active !== r.i && s.dimRect)} x={r.x} y={2} width={r.w} height={height}
                    onPointerEnter={() => setActive(r.i)} onPointerLeave={() => setActive(null)} />
                ))}
              </g>
            </svg>
            <TipLayer width={width} tip={a ? { x: a.x + a.w / 2, y: 0, title: a.seg.label, rows: [{ label: percent(a.seg.value / total), value: format(a.seg.value), tone: a.seg.tone }] } : null} />
          </>
        );
      }}
    </ChartFrame>
  );
}
