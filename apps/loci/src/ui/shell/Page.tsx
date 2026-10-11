import { useEffect, type ReactNode } from 'react';
import { Icon } from '../design/icons';
import { cx } from '../design/primitives/layer';
import s from './Page.module.css';

export interface PageProps {
  title: ReactNode;
  /** document.title (defaults to title when it is a string) */
  docTitle?: string;
  eyebrow?: ReactNode;
  subtitle?: ReactNode;
  actions?: ReactNode;
  back?: { href: string; label: string };
  width?: 'narrow' | 'default' | 'wide';
  children: ReactNode;
  className?: string;
  /** visually hide the heading (still the page's h1) */
  hideTitle?: boolean;
}

/** Standard page frame: back link, h1, subtitle, actions, content at a readable width. */
export function Page({ title, docTitle, eyebrow, subtitle, actions, back, width = 'default', children, className, hideTitle }: PageProps) {
  const dt = docTitle ?? (typeof title === 'string' ? title : undefined);
  useEffect(() => { if (dt) document.title = `${dt} · Loci`; }, [dt]);
  return (
    <div className={cx(s.page, width === 'narrow' && s.narrow, width === 'wide' && s.wide, className)}>
      <header className={cx(s.header, hideTitle && 'sr-only')}>
        <div className={s.titles}>
          {back && <a className={s.back} href={back.href}><Icon name="chevron-left" size={16} />{back.label}</a>}
          {eyebrow && <div className={s.eyebrow}>{eyebrow}</div>}
          <h1 className={s.title} data-page-title tabIndex={-1}>{title}</h1>
          {subtitle && <p className={s.subtitle}>{subtitle}</p>}
        </div>
        {actions && <div className={s.actions}>{actions}</div>}
      </header>
      {children}
    </div>
  );
}
