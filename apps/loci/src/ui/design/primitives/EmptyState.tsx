import type { ReactNode } from 'react';
import { Icon, type IconName } from '../icons';
import { cx } from './layer';
import s from './EmptyState.module.css';

export interface EmptyStateProps {
  icon?: IconName;
  title: ReactNode;
  body?: ReactNode;
  actions?: ReactNode;
  compact?: boolean;
  className?: string;
  headingLevel?: 2 | 3;
}

export function EmptyState({ icon = 'palace', title, body, actions, compact, className, headingLevel = 2 }: EmptyStateProps) {
  const H = headingLevel === 2 ? 'h2' : 'h3';
  return (
    <div className={cx(s.empty, compact && s.compact, className)}>
      <div className={s.mark} aria-hidden><Icon name={icon} size={26} /></div>
      <H className={s.title}>{title}</H>
      {body && <p className={s.body}>{body}</p>}
      {actions && <div className={s.actions}>{actions}</div>}
    </div>
  );
}
