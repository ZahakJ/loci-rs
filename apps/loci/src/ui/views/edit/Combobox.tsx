/**
 * Searchable picker (ARIA combobox + listbox). The trigger shows the current choice; opening it
 * shows a filter box over the options. Arrow keys move, Enter picks, Esc closes (without closing
 * the sheet underneath). Optional "Create …" row for a typed name that matches nothing.
 */
import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { t } from '@core/i18n/i18n';
import { Icon, type IconName } from '@ui/design/icons';
import { cx } from '@ui/design/primitives/layer';
import { floorY } from '@ui/design/primitives/bottomBar';
import s from './Combobox.module.css';

export interface ComboOption<V extends string | number> {
  value: V;
  label: string;
  /** shown muted before the label (e.g. a parent deck path) */
  prefix?: string;
  /** right-aligned detail (e.g. a count) */
  detail?: string;
  icon?: IconName;
}

export interface ComboboxProps<V extends string | number> {
  label: string;
  value: V | null;
  options: Array<ComboOption<V>>;
  onChange: (value: V) => void;
  onCreate?: (name: string) => void;
  createLabel?: (name: string) => string;
  placeholder?: string;
  icon?: IconName;
  /** hide the visible label (still labels the control) */
  hideLabel?: boolean;
  disabled?: boolean;
  className?: string;
  /** compact trigger for toolbars */
  size?: 'sm' | 'md';
  id?: string;
  /** render the popover into document.body (outside modal layers) */
  portal?: boolean;
  renderValue?: (o: ComboOption<V> | undefined) => ReactNode;
}

const norm = (s: string) => s.normalize('NFKD').replace(/\p{M}/gu, '').toLowerCase();

export function Combobox<V extends string | number>({
  label, value, options, onChange, onCreate, createLabel, placeholder, icon, hideLabel, disabled, className, size = 'md', id, portal, renderValue,
}: ComboboxProps<V>) {
  const auto = useId();
  const baseId = id ?? auto;
  const listId = `${baseId}-list`;
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [active, setActive] = useState(0);
  const [pos, setPos] = useState<{ top: number; left: number; width: number; maxHeight: number } | null>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const pop = useRef<HTMLDivElement>(null);
  const current = options.find((o) => o.value === value);

  const filtered = useMemo(() => {
    const q = norm(query.trim());
    if (!q) return options;
    const scored = options
      .map((o) => {
        const full = norm((o.prefix ? o.prefix : '') + o.label);
        const lab = norm(o.label);
        const score = lab.startsWith(q) ? 0 : lab.includes(q) ? 1 : full.includes(q) ? 2 : -1;
        return { o, score };
      })
      .filter((x) => x.score >= 0);
    return scored.sort((a, b) => a.score - b.score).map((x) => x.o);
  }, [options, query]);
  const canCreate = !!onCreate && query.trim() !== '' && !options.some((o) => norm((o.prefix ?? '') + o.label) === norm(query.trim()));
  const rows = filtered.length + (canCreate ? 1 : 0);

  const close = (focusTrigger = true) => {
    setOpen(false);
    setQuery('');
    if (focusTrigger) requestAnimationFrame(() => trigger.current?.focus());
  };
  const pick = (i: number) => {
    const o = filtered[i];
    if (o) { onChange(o.value); close(); return; }
    if (canCreate && i === filtered.length) { onCreate!(query.trim()); close(); }
  };

  useLayoutEffect(() => {
    if (!open || !trigger.current) return;
    const place = () => {
      const r = trigger.current!.getBoundingClientRect();
      const vh = floorY() + 12, vw = window.innerWidth; // keep above the phone tab bar
      const width = Math.min(Math.max(r.width, 280), vw - 16);
      const left = Math.max(8, Math.min(r.left, vw - width - 8));
      const below = vh - r.bottom - 12;
      const above = r.top - 12;
      const up = below < 240 && above > below;
      const maxHeight = Math.min(360, up ? above : below);
      setPos({ top: up ? r.top - 4 - Math.min(maxHeight, pop.current?.offsetHeight ?? maxHeight) : r.bottom + 4, left, width, maxHeight });
    };
    place();
    window.addEventListener('resize', place);
    return () => window.removeEventListener('resize', place);
  }, [open, rows]);

  useEffect(() => {
    if (!open) return;
    setActive(Math.max(0, filtered.findIndex((o) => o.value === value)));
    requestAnimationFrame(() => input.current?.focus());
    // Esc must close only this popover: run before the modal layer's capture listener.
    const onKey = (e: globalThis.KeyboardEvent) => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(); } };
    const onDown = (e: PointerEvent) => {
      const tgt = e.target as Node;
      if (pop.current?.contains(tgt) || trigger.current?.contains(tgt)) return;
      close(false);
    };
    window.addEventListener('keydown', onKey, true);
    document.addEventListener('pointerdown', onDown, true);
    return () => { window.removeEventListener('keydown', onKey, true); document.removeEventListener('pointerdown', onDown, true); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  useEffect(() => { setActive(0); }, [query]);
  useEffect(() => {
    document.getElementById(`${baseId}-opt-${active}`)?.scrollIntoView({ block: 'nearest' });
  }, [active, baseId]);

  const onInputKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'ArrowDown') { e.preventDefault(); setActive((i) => Math.min(rows - 1, i + 1)); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); setActive((i) => Math.max(0, i - 1)); }
    else if (e.key === 'Home' && e.ctrlKey) { e.preventDefault(); setActive(0); }
    else if (e.key === 'End' && e.ctrlKey) { e.preventDefault(); setActive(rows - 1); }
    else if (e.key === 'PageDown') { e.preventDefault(); setActive((i) => Math.min(rows - 1, i + 8)); }
    else if (e.key === 'PageUp') { e.preventDefault(); setActive((i) => Math.max(0, i - 8)); }
    else if (e.key === 'Enter') { e.preventDefault(); pick(active); }
    else if (e.key === 'Tab') { close(false); }
  };

  const popover = open ? (
    <div
      ref={pop}
      className={s.pop}
      style={pos ? { top: pos.top, left: pos.left, inlineSize: pos.width, maxBlockSize: pos.maxHeight } : { top: -9999, left: -9999 }}
      data-loci-keep=""
    >
      <div className={s.search}>
        <Icon name="search" size={16} />
        <input
          ref={input}
          role="combobox"
          aria-expanded="true"
          aria-controls={listId}
          aria-activedescendant={rows ? `${baseId}-opt-${active}` : undefined}
          aria-autocomplete="list"
          aria-label={label}
          className={s.input}
          value={query}
          placeholder={t('common.filter')}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onInputKey}
          autoComplete="off"
          spellCheck={false}
        />
      </div>
      <ul id={listId} role="listbox" aria-label={label} className={s.list}>
        {filtered.map((o, i) => (
          <li
            key={String(o.value)}
            id={`${baseId}-opt-${i}`}
            role="option"
            aria-selected={o.value === value}
            data-active={i === active || undefined}
            className={s.option}
            onPointerMove={() => setActive(i)}
            onClick={() => pick(i)}
          >
            {o.icon && <Icon name={o.icon} size={16} />}
            <span className={s.optLabel}>
              {o.prefix && <span className={s.prefix}>{o.prefix}</span>}
              {o.label}
            </span>
            {o.detail && <span className={s.detail}>{o.detail}</span>}
            {o.value === value && <Icon name="check" size={16} className={s.check} />}
          </li>
        ))}
        {canCreate && (
          <li
            id={`${baseId}-opt-${filtered.length}`}
            role="option"
            aria-selected={false}
            data-active={active === filtered.length || undefined}
            className={cx(s.option, s.create)}
            onPointerMove={() => setActive(filtered.length)}
            onClick={() => pick(filtered.length)}
          >
            <Icon name="plus" size={16} />
            <span className={s.optLabel}>{createLabel ? createLabel(query.trim()) : query.trim()}</span>
          </li>
        )}
        {rows === 0 && <li className={s.empty} role="presentation">{t('common.noMatches')}</li>}
      </ul>
    </div>
  ) : null;

  return (
    <div className={cx(s.root, size === 'sm' && s.sm, className)}>
      <span id={`${baseId}-label`} className={cx(s.label, hideLabel && 'sr-only')}>{label}</span>
      <button
        ref={trigger}
        type="button"
        id={baseId}
        className={s.trigger}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-labelledby={`${baseId}-label ${baseId}`}
        disabled={disabled}
        onClick={() => (open ? close() : setOpen(true))}
        onKeyDown={(e) => {
          if ((e.key === 'ArrowDown' || e.key === 'ArrowUp') && !open) { e.preventDefault(); setOpen(true); }
          else if (e.key.length === 1 && /\S/.test(e.key) && !e.ctrlKey && !e.metaKey && !e.altKey && !open) { setQuery(e.key); setOpen(true); }
        }}
      >
        {icon && <Icon name={icon} size={16} className={s.triggerIcon} />}
        <span className={s.value}>
          {renderValue ? renderValue(current) : current ? (
            <>
              {current.prefix && <span className={s.prefix}>{current.prefix}</span>}
              {current.label}
            </>
          ) : <span className={s.placeholder}>{placeholder}</span>}
        </span>
        <Icon name="chevron-down" size={16} className={s.chevron} />
      </button>
      {popover && (portal ? createPortal(popover, document.body) : popover)}
    </div>
  );
}

/** Options for decks: leaf name with the parent path as a muted prefix. */
export function deckOptions(decks: Array<{ id: number; name: string }>): Array<ComboOption<number>> {
  return [...decks]
    .sort((a, b) => a.name.localeCompare(b.name))
    .map((d) => {
      const parts = d.name.split('::');
      const leaf = parts.pop() ?? d.name;
      return { value: d.id, label: leaf, prefix: parts.length ? `${parts.join(' / ')} / ` : undefined };
    });
}
