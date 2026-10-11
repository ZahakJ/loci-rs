/** Columns chooser: a menu of checkable columns plus the sort column/direction for the mode. */
import { useEffect, useId, useRef, useState, type KeyboardEvent } from 'react';
import { t } from '@core/i18n/i18n';
import type { BrowseColumn, BrowseMode } from '@core/types';
import { Icon } from '@ui/design/icons';
import { Button } from '@ui/design/primitives/Button';
import { ALL_COLUMNS, CARD_ONLY, NOTE_ONLY } from './columns';
import s from './ColumnsMenu.module.css';

export interface ColumnsMenuProps {
  mode: BrowseMode;
  columns: BrowseColumn[];
  onToggle: (c: BrowseColumn) => void;
  onReset: () => void;
  iconOnly?: boolean;
}

export function ColumnsMenu({ mode, columns, onToggle, onReset, iconOnly }: ColumnsMenuProps) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const btn = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const id = useId();
  const available = ALL_COLUMNS.filter((c) => (mode === 'notes' ? !CARD_ONLY.has(c) : !NOTE_ONLY.has(c)));

  useEffect(() => {
    if (!open) return;
    requestAnimationFrame(() => menu.current?.querySelector<HTMLElement>(`[data-i="${active}"]`)?.focus());
    const onDown = (e: PointerEvent) => {
      const tg = e.target as Node;
      if (menu.current?.contains(tg) || btn.current?.contains(tg)) return;
      setOpen(false);
    };
    const onKey = (e: globalThis.KeyboardEvent) => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); setOpen(false); btn.current?.focus(); } };
    document.addEventListener('pointerdown', onDown, true);
    window.addEventListener('keydown', onKey, true);
    return () => { document.removeEventListener('pointerdown', onDown, true); window.removeEventListener('keydown', onKey, true); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const move = (i: number) => {
    const n = available.length + 1;
    const next = (i + n) % n;
    setActive(next);
    menu.current?.querySelector<HTMLElement>(`[data-i="${next}"]`)?.focus();
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key === 'ArrowDown') { e.preventDefault(); move(active + 1); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); move(active - 1); }
    else if (e.key === 'Home') { e.preventDefault(); move(0); }
    else if (e.key === 'End') { e.preventDefault(); move(available.length); }
    else if (e.key === 'Tab') setOpen(false);
  };

  return (
    <div className={s.wrap}>
      <Button
        ref={btn}
        size="sm"
        variant="ghost"
        icon="columns"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        aria-label={t('browse.columnsMenu')}
        onClick={() => setOpen((o) => !o)}
      >
        {iconOnly ? null : t('browse.columnsMenu')}
      </Button>
      {open && (
        <div ref={menu} id={id} role="menu" aria-label={t('browse.columnsMenu')} className={s.menu} onKeyDown={onKey}>
          {available.map((c, i) => {
            const on = columns.includes(c);
            return (
              <button
                key={c}
                type="button"
                role="menuitemcheckbox"
                aria-checked={on}
                tabIndex={i === active ? 0 : -1}
                data-i={i}
                className={s.item}
                onClick={() => onToggle(c)}
                onFocus={() => setActive(i)}
              >
                <span className={s.box} aria-hidden>{on && <Icon name="check" size={14} />}</span>
                {t(`browse.columns.${c}`)}
              </button>
            );
          })}
          <div role="separator" className={s.sep} />
          <button type="button" role="menuitem" tabIndex={active === available.length ? 0 : -1} data-i={available.length} className={s.item} onClick={() => { onReset(); setOpen(false); }} onFocus={() => setActive(available.length)}>
            <span className={s.box} aria-hidden><Icon name="undo" size={14} /></span>
            {t('browse.columnsReset')}
          </button>
        </div>
      )}
    </div>
  );
}
