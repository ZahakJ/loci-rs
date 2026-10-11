import { cx } from './layer';
import s from './Sparkline.module.css';

export interface SparklineProps {
  values: number[];
  width?: number;
  height?: number;
  /** accessible description; if absent the sparkline is decorative */
  label?: string;
  tone?: 'accent' | 'muted' | 'new' | 'due';
  area?: boolean;
  /** stretch to the container width */
  fluid?: boolean;
  className?: string;
}

export function Sparkline({ values, width = 120, height = 32, label, tone = 'accent', area = true, fluid, className }: SparklineProps) {
  const n = values.length;
  if (n === 0) return null;
  const max = Math.max(...values, 1e-9), min = Math.min(...values, 0);
  const span = max - min || 1;
  const pad = 2;
  const x = (i: number) => (n === 1 ? width / 2 : (i / (n - 1)) * width);
  const y = (v: number) => pad + (1 - (v - min) / span) * (height - pad * 2);
  const pts = values.map((v, i) => `${x(i).toFixed(2)},${y(v).toFixed(2)}`).join(' ');
  const last = values[n - 1]!;
  return (
    <svg
      className={cx(s.svg, tone !== 'accent' && s[tone], className)}
      viewBox={`0 0 ${width} ${height}`}
      width={fluid ? '100%' : width}
      height={height}
      preserveAspectRatio="none"
      role={label ? 'img' : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    >
      {area && <polygon className={s.area} points={`0,${height} ${pts} ${width},${height}`} />}
      <polyline className={s.line} points={pts} />
      {!fluid && <circle className={s.dot} cx={x(n - 1)} cy={y(last)} r={2} />}
    </svg>
  );
}
