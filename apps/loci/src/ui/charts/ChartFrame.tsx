/**
 * Shared chart chrome: measures its width (ResizeObserver), hosts the hover/focus tooltip, and
 * renders the visually-hidden data table every chart carries (DESIGN §16; dataviz: tooltips
 * enhance, never gate). The SVG itself is `role="img"` with <title>/<desc>.
 */
import { useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import { cx } from '@ui/design/primitives/layer';
import s from './charts.module.css';

export type Tone = 'accent' | 'new' | 'learn' | 'due' | 'young' | 'mature' | 'again' | 'hard' | 'good' | 'easy' | 'muted' | 'faint';

export interface TipRow { label: string; value: string; tone?: Tone; line?: boolean }
export interface Tip { x: number; y: number; title: string; rows: TipRow[] }

export interface TableData { caption: string; headers: string[]; rows: Array<Array<string | number>> }

/** Width of an element, kept current with a ResizeObserver. */
export function useWidth<T extends HTMLElement>(fallback = 600): [React.RefObject<T | null>, number] {
  const ref = useRef<T>(null);
  const [w, setW] = useState(fallback);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    setW(el.clientWidth || fallback);
    const ro = new ResizeObserver(([e]) => { const cw = Math.round(e?.contentRect.width ?? 0); if (cw > 0) setW(cw); });
    ro.observe(el);
    return () => ro.disconnect();
  }, [fallback]);
  return [ref, w];
}

export interface ChartFrameProps {
  title: string;
  desc: string;
  table: TableData;
  /** keyboard: arrow keys step through data points */
  count?: number;
  active?: number | null;
  onActive?: (i: number | null) => void;
  className?: string;
  children: (width: number, ids: { title: string; desc: string }) => ReactNode;
  legend?: ReactNode;
}

export function ChartFrame({ title, desc, table, count = 0, active, onActive, className, children, legend }: ChartFrameProps) {
  const [ref, width] = useWidth<HTMLDivElement>();
  const id = useId();
  const ids = { title: `${id}-t`, desc: `${id}-d` };
  const onKey = (e: KeyboardEvent) => {
    if (!onActive || !count) return;
    const cur = active ?? -1;
    const rtl = document.documentElement.dir === 'rtl';
    const next = e.key === (rtl ? 'ArrowLeft' : 'ArrowRight') ? cur + 1 : e.key === (rtl ? 'ArrowRight' : 'ArrowLeft') ? cur - 1 : e.key === 'Home' ? 0 : e.key === 'End' ? count - 1 : null;
    if (next === null) { if (e.key === 'Escape') onActive(null); return; }
    e.preventDefault();
    onActive(Math.max(0, Math.min(count - 1, next < 0 ? count - 1 : next)));
  };
  return (
    <div className={cx(s.frame, className)}>
      <div
        ref={ref}
        className={s.plot}
        tabIndex={count ? 0 : undefined}
        onKeyDown={onKey}
        onBlur={() => onActive?.(null)}
        aria-describedby={ids.desc}
        aria-label={count ? title : undefined}
        role={count ? 'group' : undefined}
      >
        {children(width, ids)}
      </div>
      {legend}
      <div className="sr-only">
      <table>
        <caption>{table.caption}</caption>
        <thead><tr>{table.headers.map((h) => <th key={h} scope="col">{h}</th>)}</tr></thead>
        <tbody>{table.rows.map((r, i) => <tr key={i}>{r.map((c, j) => (j === 0 ? <th key={j} scope="row">{c}</th> : <td key={j}>{c}</td>))}</tr>)}</tbody>
      </table>
      </div>
    </div>
  );
}

export interface LegendItem { label: string; tone: Tone; value?: string; line?: boolean }
export function Legend({ items, className }: { items: LegendItem[]; className?: string }) {
  return (
    <ul role="list" className={cx(s.legend, className)} aria-hidden>
      {items.map((it) => (
        <li key={it.label} className={s.legendItem}>
          <span className={cx(s.swatch, s[`tone-${it.tone}`], it.line && s.keyLine)} />
          <span>{it.label}</span>
          {it.value && <span className={s.legendValue}>{it.value}</span>}
        </li>
      ))}
    </ul>
  );
}
