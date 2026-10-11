import type { CSSProperties, Ref } from 'react';
import { useFieldControl } from './Field';
import { cx } from './layer';
import s from './Slider.module.css';

export interface SliderProps {
  value: number;
  onChange: (value: number) => void;
  min: number;
  max: number;
  step?: number;
  /** formats the visible value and aria-valuetext */
  format?: (v: number) => string;
  /** show the value label above the track (default true) */
  showValue?: boolean;
  /** show min/max labels under the track */
  showScale?: boolean;
  label?: string;
  disabled?: boolean;
  id?: string;
  className?: string;
  onCommit?: (value: number) => void;
  ref?: Ref<HTMLInputElement>;
}

/** Native range input, styled, with a live value label. */
export function Slider({ value, onChange, min, max, step = 1, format = String, showValue = true, showScale, label, disabled, id, className, onCommit, ref }: SliderProps) {
  const a11y = useFieldControl(id);
  const pct = ((value - min) / (max - min || 1)) * 100;
  return (
    <div className={cx(s.wrap, className)}>
      {showValue && (
        <div className={s.top}>
          <output htmlFor={a11y.id} className={cx(s.value, 'num')}>{format(value)}</output>
        </div>
      )}
      <input
        ref={ref}
        type="range"
        {...a11y}
        aria-label={label}
        aria-valuetext={format(value)}
        className={s.range}
        style={{ '--pct': `${pct}%` } as CSSProperties}
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(Number(e.target.value))}
        onPointerUp={(e) => onCommit?.(Number((e.target as HTMLInputElement).value))}
        onKeyUp={(e) => onCommit?.(Number((e.target as HTMLInputElement).value))}
      />
      {showScale && <div className={s.scale} aria-hidden><span>{format(min)}</span><span>{format(max)}</span></div>}
    </div>
  );
}
