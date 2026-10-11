import type { HTMLAttributes, ElementType, Ref } from 'react';
import { cx } from './layer';
import s from './Card.module.css';

export interface CardProps extends HTMLAttributes<HTMLElement> {
  as?: ElementType;
  padding?: 'none' | 'sm' | 'md';
  interactive?: boolean;
  tone?: 'surface' | 'sunken';
  ref?: Ref<HTMLElement>;
}

/** The surface panel: 1px line border, radius 16, no shadow. */
export function Card({ as: As = 'div', padding = 'md', interactive, tone = 'surface', className, ...rest }: CardProps) {
  return <As className={cx(s.card, s[`pad-${padding}`], interactive && s.interactive, tone === 'sunken' && s.sunken, className)} {...rest} />;
}
