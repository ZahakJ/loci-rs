/** Progress ring for a single share (e.g. today's correct answers), with the value in the middle. */
import { cx } from '@ui/design/primitives/layer';
import type { Tone } from './ChartFrame';
import s from './charts.module.css';

export interface RingProps {
  /** 0..1 */
  value: number;
  label: string;
  /** text in the centre; default the percentage */
  center?: string;
  size?: number;
  tone?: Tone;
  /** optional target marker (0..1) */
  target?: number;
  className?: string;
}

export function Ring({ value, label, center, size = 96, tone = 'accent', target, className }: RingProps) {
  const v = Math.max(0, Math.min(1, Number.isFinite(value) ? value : 0));
  const stroke = 8;
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const angle = (target ?? 0) * 2 * Math.PI - Math.PI / 2;
  return (
    <svg className={cx(s.ring, className)} width={size} height={size} viewBox={`0 0 ${size} ${size}`} role="img" aria-label={label}>
      <circle className={s.ringTrack} cx={size / 2} cy={size / 2} r={r} strokeWidth={stroke} />
      <circle className={cx(s.ringValue, s[`stroke-${tone}`])} cx={size / 2} cy={size / 2} r={r} strokeWidth={stroke}
        strokeDasharray={`${c * v} ${c}`} transform={`rotate(-90 ${size / 2} ${size / 2})`} />
      {target !== undefined && (
        <line className={s.ringTarget} x1={size / 2 + Math.cos(angle) * (r - stroke)} y1={size / 2 + Math.sin(angle) * (r - stroke)} x2={size / 2 + Math.cos(angle) * (r + stroke)} y2={size / 2 + Math.sin(angle) * (r + stroke)} />
      )}
      <text className={s.ringText} x="50%" y="50%" dominantBaseline="central" textAnchor="middle" aria-hidden>{center ?? `${Math.round(v * 100)}%`}</text>
    </svg>
  );
}
