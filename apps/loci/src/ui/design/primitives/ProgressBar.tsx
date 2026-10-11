import { cx } from './layer';
import s from './ProgressBar.module.css';

export interface ProgressBarProps {
  /** 0..1; undefined = indeterminate */
  value?: number;
  label?: string;
  /** right-aligned detail, e.g. "1,204 / 8,000" */
  detail?: string;
  thin?: boolean;
  className?: string;
}

export function ProgressBar({ value, label, detail, thin, className }: ProgressBarProps) {
  const indeterminate = value === undefined || !Number.isFinite(value);
  const v = indeterminate ? 0 : Math.max(0, Math.min(1, value));
  return (
    <div className={cx(s.wrap, thin && s.thin, indeterminate && s.indeterminate, className)}>
      {(label || detail) && (
        <div className={s.meta}>
          {label && <span className={s.label}>{label}</span>}
          {detail && <span className="tabular">{detail}</span>}
        </div>
      )}
      <div
        className={s.track}
        role="progressbar"
        aria-label={label ?? 'Progress'}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={indeterminate ? undefined : Math.round(v * 100)}
        aria-valuetext={detail}
      >
        <div className={s.fill} style={indeterminate ? undefined : { transform: `scaleX(${v})` }} />
      </div>
    </div>
  );
}
