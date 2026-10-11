import type { ReactNode } from 'react';
import { cx } from './layer';
import s from './Pill.module.css';

export type PillTone = 'new' | 'learn' | 'due' | 'neutral' | 'accent' | 'again' | 'hard' | 'good' | 'easy';

export interface PillProps {
  tone?: PillTone;
  /** a count; 0 renders as a faint dash. Always set in the data numeral face (`.num`). */
  value?: number;
  children?: ReactNode;
  dot?: boolean;
  outline?: boolean;
  size?: 'md' | 'lg';
  className?: string;
  /** accessible text when the visual is terse */
  label?: string;
  /** set the children in the data numeral face too (default: when the children are a number) */
  num?: boolean;
}

export function Pill({ tone = 'neutral', value, children, dot, outline, size = 'md', className, label, num }: PillProps) {
  const zero = value === 0;
  const numericChildren = num ?? typeof children === 'number';
  return (
    <span className={cx(s.pill, s[tone], zero && s.zero, outline && s.outline, size === 'lg' && s.lg, className)}>
      {dot && !zero && <span className={s.dot} aria-hidden />}
      <span aria-hidden={label ? true : undefined}>
        {value !== undefined && <span className="num">{zero ? '–' : value.toLocaleString()}</span>}
        {numericChildren ? <span className="num">{children}</span> : children}
      </span>
      {label && <span className="sr-only">{label}</span>}
    </span>
  );
}
