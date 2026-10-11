/**
 * Line chart over ordered categories: 2px lines, optional 10% area wash, end dot with a surface
 * ring, crosshair that snaps to the nearest point, one tooltip for every series, optional
 * reference line. One y-axis only.
 */
import { useState, type PointerEvent } from 'react';
import { cx } from '@ui/design/primitives/layer';
import { TipLayer } from './Bars';
import { ChartFrame, Legend, type TableData, type Tip, type Tone } from './ChartFrame';
import { compactNumber, labelIndices, linearScale } from './scale';
import s from './charts.module.css';

export interface LineSeries { key: string; label: string; tone: Tone; area?: boolean }
export interface LinePoint { label: string; values: Array<number | null> }

export interface LinesProps {
  data: LinePoint[];
  series: LineSeries[];
  height?: number;
  title: string;
  desc: string;
  format?: (v: number) => string;
  yTick?: (v: number) => string;
  yMin?: number;
  yMax?: number;
  maxXLabels?: number;
  refLine?: { value: number; label: string };
  categoryHeader: string;
  legend?: boolean;
  className?: string;
}

const AXIS_W = 40, AXIS_H = 22, TOP = 10, PAD = 8;

export function Lines({ data, series, height = 160, title, desc, format = (v) => v.toLocaleString(), yTick, yMin, yMax, maxXLabels = 7, refLine, categoryHeader, legend = series.length > 1, className }: LinesProps) {
  const [active, setActive] = useState<number | null>(null);
  const all = data.flatMap((d) => d.values.filter((v): v is number => v !== null));
  const table: TableData = { caption: title, headers: [categoryHeader, ...series.map((x) => x.label)], rows: data.map((d) => [d.label, ...d.values.map((v) => (v === null ? '–' : format(v)))]) };
  return (
    <ChartFrame title={title} desc={desc} table={table} count={data.length} active={active} onActive={setActive} className={className}
      legend={legend ? <Legend items={series.map((x) => ({ label: x.label, tone: x.tone, line: true }))} /> : undefined}>
      {(width, ids) => {
        const plotW = Math.max(40, width - AXIS_W - PAD);
        const y = linearScale([...all, refLine?.value ?? all[0] ?? 0], [TOP + height, TOP], { min: yMin, max: yMax, zero: yMin === undefined ? true : undefined });
        const n = data.length;
        const x = (i: number) => AXIS_W + PAD / 2 + (n <= 1 ? plotW / 2 : (i / (n - 1)) * plotW);
        const labels = new Set(labelIndices(n, Math.max(2, Math.min(maxXLabels, Math.floor(plotW / 64)))));
        const tick = yTick ?? ((v: number) => compactNumber(v));
        const onMove = (e: PointerEvent<SVGRectElement>) => {
          const r = e.currentTarget.getBoundingClientRect();
          const px = e.clientX - r.left;
          const i = n <= 1 ? 0 : Math.round((px / r.width) * (n - 1));
          setActive(Math.max(0, Math.min(n - 1, i)));
        };
        const paths = series.map((sr, k) => {
          let d = '', area = '';
          let start = -1, last = -1;
          data.forEach((p, i) => {
            const v = p.values[k];
            if (v === null || v === undefined) return;
            d += `${start < 0 || last !== i - 1 ? 'M' : 'L'}${x(i).toFixed(1)},${y(v).toFixed(1)}`;
            if (start < 0) start = i;
            last = i;
          });
          if (sr.area && start >= 0) {
            area = `M${x(start)},${y(Math.max(0, y.domain[0]))}` + data.map((p, i) => (p.values[k] === null || i < start || i > last ? '' : `L${x(i).toFixed(1)},${y(p.values[k]!).toFixed(1)}`)).join('') + `L${x(last)},${y(Math.max(0, y.domain[0]))}Z`;
          }
          return { d, area, last };
        });
        const tip: Tip | null = active !== null && data[active] ? {
          x: x(active), y: TOP + 4,
          title: data[active]!.label,
          rows: series.map((sr, k) => ({ label: sr.label, value: data[active]!.values[k] === null ? '–' : format(data[active]!.values[k]!), tone: sr.tone, line: true })),
        } : null;
        return (
          <>
            <svg width={width} height={TOP + height + AXIS_H} className={s.svg} role="img" aria-labelledby={ids.title} aria-describedby={ids.desc}>
              <title id={ids.title}>{title}</title>
              <desc id={ids.desc}>{desc}</desc>
              <g aria-hidden>
                {y.ticks.map((tv) => (
                  <g key={tv}>
                    <line className={s.grid} x1={AXIS_W} x2={AXIS_W + plotW + PAD} y1={Math.round(y(tv)) + 0.5} y2={Math.round(y(tv)) + 0.5} />
                    <text className={s.yTick} x={AXIS_W - 8} y={y(tv) + 4} textAnchor="end">{tick(tv)}</text>
                  </g>
                ))}
                {data.map((p, i) => labels.has(i) && <text key={i} className={s.xTick} x={x(i)} y={TOP + height + 16} textAnchor="middle">{p.label}</text>)}
                {refLine && (
                  <g className={s.ref}>
                    <line x1={AXIS_W} x2={AXIS_W + plotW + PAD} y1={y(refLine.value)} y2={y(refLine.value)} />
                    <text x={AXIS_W + plotW + PAD} y={y(refLine.value) - 6} textAnchor="end">{refLine.label}</text>
                  </g>
                )}
                {paths.map((p, k) => p.area && <path key={`a${k}`} className={cx(s.area, s[`fill-${series[k]!.tone}`])} d={p.area} />)}
                {paths.map((p, k) => <path key={`l${k}`} className={cx(s.line, s[`stroke-${series[k]!.tone}`])} d={p.d} />)}
                {paths.map((p, k) => p.last >= 0 && active === null && (
                  <circle key={`e${k}`} className={cx(s.dot, s[`fill-${series[k]!.tone}`])} cx={x(p.last)} cy={y(data[p.last]!.values[k]!)} r={4} />
                ))}
                {active !== null && (
                  <>
                    <line className={s.crosshair} x1={x(active)} x2={x(active)} y1={TOP} y2={TOP + height} />
                    {series.map((sr, k) => data[active]!.values[k] !== null && (
                      <circle key={k} className={cx(s.dot, s[`fill-${sr.tone}`])} cx={x(active)} cy={y(data[active]!.values[k]!)} r={4} />
                    ))}
                  </>
                )}
                <rect className={s.hit} x={AXIS_W} y={TOP} width={plotW + PAD} height={height} onPointerMove={onMove} onPointerLeave={() => setActive(null)} />
              </g>
            </svg>
            <TipLayer tip={tip} width={width} />
          </>
        );
      }}
    </ChartFrame>
  );
}
