import { useRef, type KeyboardEvent, type ReactNode } from 'react';
import { Icon, type IconName } from '../icons';
import { cx } from './layer';
import s from './Segmented.module.css';

export interface SegmentedOption<V extends string> { value: V; label: ReactNode; icon?: IconName; disabled?: boolean; ariaLabel?: string }
export interface SegmentedProps<V extends string> {
  options: Array<SegmentedOption<V>>;
  value: V | null;
  onChange: (value: V) => void;
  /** accessible name of the group */
  label: string;
  block?: boolean;
  className?: string;
  id?: string;
}

/** Radio group styled as a segmented control. Arrow keys move and select. */
export function Segmented<V extends string>({ options, value, onChange, label, block, className, id }: SegmentedProps<V>) {
  const refs = useRef<Array<HTMLButtonElement | null>>([]);
  const current = Math.max(0, options.findIndex((o) => o.value === value));
  const onKey = (e: KeyboardEvent, i: number) => {
    const rtl = document.documentElement.dir === 'rtl';
    const fwd = rtl ? 'ArrowLeft' : 'ArrowRight';
    const back = rtl ? 'ArrowRight' : 'ArrowLeft';
    let next = -1;
    if (e.key === fwd || e.key === 'ArrowDown') next = i + 1;
    else if (e.key === back || e.key === 'ArrowUp') next = i - 1;
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = options.length - 1;
    if (next === -1) return;
    e.preventDefault();
    const n = options.length;
    for (let k = 0; k < n; k++) {
      const idx = (((next + (next < i ? -k : k)) % n) + n) % n;
      const o = options[idx]!;
      if (!o.disabled) { onChange(o.value); refs.current[idx]?.focus(); break; }
    }
  };
  return (
    <div role="radiogroup" aria-label={label} id={id} className={cx(s.group, block && s.block, className)}>
      {options.map((o, i) => (
        <button
          key={o.value}
          ref={(el) => { refs.current[i] = el; }}
          type="button"
          role="radio"
          aria-checked={o.value === value}
          aria-label={o.ariaLabel}
          tabIndex={i === current ? 0 : -1}
          disabled={o.disabled}
          className={s.option}
          onClick={() => onChange(o.value)}
          onKeyDown={(e) => onKey(e, i)}
        >
          {o.icon && <Icon name={o.icon} size={16} />}
          {o.label}
        </button>
      ))}
    </div>
  );
}
