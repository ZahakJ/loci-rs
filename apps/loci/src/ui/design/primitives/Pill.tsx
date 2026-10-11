import type { ReactNode } from 'react';
import { cx } from './layer';
import s from './Pill.module.css';

export type PillTone = 'new' | 'learn' | 'due' | 'neutral' | 'accent' | 'again' | 'hard' | 'good' | 'easy';

export interface PillProps {
  tone?: PillTone;
  /** a count; 0 renders as a faint dash */
  value?: number;
  children?: ReactNode;
  dot?: boolean;
  outline?: boolean;
  size?: 'md' | 'lg';
  className?: string;
  /** accessible text when the visual is terse */
  label?: string;
  title?: string;
}

export function Pill({ tone = 'neutral', value, children, dot, outline, size = 'md', className, label, title }: PillProps) {
  const zero = value === 0;
  return (
    <span className={cx(s.pill, s[tone], zero && s.zero, outline && s.outline, size === 'lg' && s.lg, className)} title={title}>
      {dot && !zero && <span className={s.dot} aria-hidden />}
      <span aria-hidden={label ? true : undefined}>{value !== undefined ? (zero ? '–' : value.toLocaleString()) : null}{children}</span>
      {label && <span className="sr-only">{label}</span>}
    </span>
  );
}
