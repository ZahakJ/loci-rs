import { useEffect, useRef, useState, type ReactNode } from 'react';
import { t } from '@core/i18n/i18n';
import { Icon } from '../../design/icons';
import { cx } from '../../design/primitives/layer';
import s from './Settings.module.css';

export function SettingRow({ label, help, children, stack, inline, id, extra }: {
  label: ReactNode; help?: ReactNode; children: ReactNode; stack?: boolean; inline?: boolean; id?: string; extra?: ReactNode;
}) {
  return (
    <div className={cx(s.row, stack && s.stackRow, inline && s.inlineRow)} id={id}>
      <div className={s.text}>
        <div className={s.label}>{label}{extra}</div>
        {help && <div className={s.help}>{help}</div>}
      </div>
      <div className={cx(s.control, stack && s.wideControl)}>{children}</div>
    </div>
  );
}

export function Section({ id, title, lede, status, children }: { id: string; title: string; lede?: ReactNode; status?: ReactNode; children: ReactNode }) {
  return (
    <section id={`settings-${id}`} className={s.section} aria-labelledby={`settings-${id}-title`}>
      <div className={s.sectionHead}>
        <h2 id={`settings-${id}-title`} className={s.sectionTitle}>{title}</h2>
        {status}
      </div>
      {lede && <p className={s.sectionLede}>{lede}</p>}
      {children}
    </section>
  );
}

/** "Saved" note that appears briefly after a write (polite live region). */
export function SavedNote({ at }: { at: number }) {
  const [show, setShow] = useState(false);
  useEffect(() => {
    if (!at) return;
    setShow(true);
    const id = window.setTimeout(() => setShow(false), 1800);
    return () => window.clearTimeout(id);
  }, [at]);
  return (
    <span className={s.saved} aria-live="polite">
      {show && <><Icon name="check" size={14} />{t('settings.saved')}</>}
    </span>
  );
}

/** Sticky in-page index for long settings pages (desktop). Highlights the section in view. */
export function SideIndex({ items, label, hrefFor, ready = true }: {
  items: Array<{ id: string; label: string }>; label: string; hrefFor: (id: string) => string;
  /** false while the sections are still loading (they are observed once they exist) */
  ready?: boolean;
}) {
  const [active, setActive] = useState(items[0]?.id);
  const pinned = useRef(0);
  useEffect(() => {
    if (!ready) return;
    const els = items.map((i) => document.getElementById(`settings-${i.id}`)).filter((e): e is HTMLElement => !!e);
    if (els.length === 0) return;
    const pick = () => {
      if (Date.now() < pinned.current) return; // a click just chose the section; let the smooth scroll land
      // the first section whose top is in the upper part of the viewport, else the last one above it
      const line = window.innerHeight * 0.3;
      let best: HTMLElement | undefined;
      for (const el of els) { if (el.getBoundingClientRect().top <= line) best = el; }
      const atEnd = window.innerHeight + window.scrollY >= document.documentElement.scrollHeight - 4;
      if (atEnd) best = els[els.length - 1];
      const chosen = best ?? els[0]!;
      setActive(chosen.id.replace('settings-', ''));
    };
    // the observer wakes `pick` as sections cross the viewport; scroll covers long sections
    const io = new IntersectionObserver(pick, { threshold: [0, 0.25, 0.5, 1], rootMargin: '0px 0px -40% 0px' });
    els.forEach((e) => io.observe(e));
    window.addEventListener('scroll', pick, { passive: true });
    pick();
    return () => { io.disconnect(); window.removeEventListener('scroll', pick); };
  }, [items, ready]);
  return (
    <nav className={s.index} aria-label={label}>
      <ul role="list" className={s.indexList}>
        {items.map((i) => (
          <li key={i.id}>
            <a
              className={s.indexLink}
              href={hrefFor(i.id)}
              aria-current={active === i.id ? 'true' : undefined}
              onClick={(e) => {
                e.preventDefault();
                const el = document.getElementById(`settings-${i.id}`);
                el?.scrollIntoView({ behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth', block: 'start' });
                pinned.current = Date.now() + 700;
                setActive(i.id);
              }}
            >
              {i.label}
            </a>
          </li>
        ))}
      </ul>
    </nav>
  );
}
