import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState, type ButtonHTMLAttributes, type KeyboardEvent, type Ref } from 'react';
import { createPortal } from 'react-dom';
import { Icon } from '../icons';
import { floorY } from './bottomBar';
import { useFieldControl } from './Field';
import { cx } from './layer';
import { firstEnabled, listKey, typeahead, type Typeahead } from './listNav';
import s from './Select.module.css';

export interface SelectOption<V extends string | number = string> { value: V; label: string; disabled?: boolean }
export interface SelectProps<V extends string | number = string>
  extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'onChange' | 'value' | 'title' | 'children' | 'type'> {
  options: Array<SelectOption<V>>;
  value: V;
  onChange: (value: V) => void;
  ref?: Ref<HTMLButtonElement>;
}

/**
 * A designed single-choice picker (ARIA select-only combobox): a button showing the current label
 * and a chevron, opening a listbox popover styled like Menu. Arrow keys, Home/End, PageUp/PageDown
 * and typeahead move; Enter, Space or a click picks; Esc and a click outside close; Tab closes and
 * moves on. The popover flips above the trigger when there is no room below and never covers the
 * phone tab bar or the study answer bar.
 */
export function Select<V extends string | number = string>({ options, value, onChange, className, id, disabled, ref, onKeyDown, onClick, ...rest }: SelectProps<V>) {
  const a11y = useFieldControl(id);
  const listId = useId();
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(-1);
  const [pos, setPos] = useState<{ top: number; left: number; width: number; maxHeight: number } | null>(null);
  const trigger = useRef<HTMLButtonElement | null>(null);
  const list = useRef<HTMLDivElement>(null);
  const ta = useRef<Typeahead>({ text: '', at: 0 });
  const selected = options.findIndex((o) => o.value === value);
  const current = options[selected];

  const close = useCallback((focusTrigger: boolean) => {
    setOpen(false);
    setPos(null);
    if (focusTrigger) trigger.current?.focus({ preventScroll: true });
  }, []);

  const openAt = (index: number) => {
    if (disabled) return;
    setActive(index >= 0 ? index : Math.max(0, firstEnabled(options, 0, 1)));
    setOpen(true);
  };

  const pick = (index: number) => {
    const o = options[index];
    if (!o || o.disabled) return;
    close(true);
    if (o.value !== value) onChange(o.value);
  };

  useLayoutEffect(() => {
    if (!open || !trigger.current || !list.current) return;
    const place = () => {
      const r = trigger.current!.getBoundingClientRect();
      const natural = list.current!.scrollHeight;
      const vw = window.innerWidth;
      const floor = floorY();
      const width = Math.min(Math.max(r.width, 160), vw - 16);
      const rtl = getComputedStyle(trigger.current!).direction === 'rtl';
      let left = rtl ? r.right - width : r.left;
      left = Math.max(8, Math.min(left, vw - width - 8));
      const below = floor - r.bottom - 4;
      const above = r.top - 8 - 4;
      const up = below < Math.min(natural, 240) && above > below;
      const maxHeight = Math.max(96, Math.min(360, up ? above : below));
      const h = Math.min(natural, maxHeight);
      setPos({ top: up ? r.top - 4 - h : r.bottom + 4, left, width, maxHeight });
    };
    place();
    // The page may still be scrolling (a section link, a sheet opening): follow the trigger.
    const onScroll = (e: Event) => { if (!list.current?.contains(e.target as Node)) place(); };
    window.addEventListener('resize', place);
    return () => { window.removeEventListener('resize', place); window.removeEventListener('scroll', onScroll, true); };
  }, [open, options.length]);

  useEffect(() => {
    if (!open) return;
    // Window capture runs before the modal layer's document listener: Esc closes only this list,
    // and Tab leaves from the trigger, as it does from a native select.
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (!list.current?.contains(document.activeElement)) return;
      if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(true); }
      else if (e.key === 'Tab') close(true);
    };
    const onDown = (e: PointerEvent) => {
      const t = e.target as Node;
      if (list.current?.contains(t) || trigger.current?.contains(t)) return;
      close(false);
    };
    window.addEventListener('keydown', onKey, true);
    document.addEventListener('pointerdown', onDown, true);
    return () => {
      window.removeEventListener('keydown', onKey, true);
      document.removeEventListener('pointerdown', onDown, true);
    };
  }, [open, close]);

  // Roving focus: the active option holds DOM focus once the list is placed.
  useEffect(() => {
    if (!open || !pos) return;
    const el = document.getElementById(`${listId}-${active}`) ?? list.current;
    el?.focus({ preventScroll: true });
    el?.scrollIntoView?.({ block: 'nearest' });
  }, [open, pos, active, listId]);

  const onListKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const a = listKey(options, active, e.key, ta.current, Date.now(), { alt: e.altKey, ctrl: e.ctrlKey, meta: e.metaKey });
    if (a.kind === 'none' || a.kind === 'tab') return;
    e.preventDefault();
    if (a.kind === 'move') setActive(a.to);
    else if (a.kind === 'pick') pick(a.index);
    else close(true);
  };

  const onTriggerKey = (e: KeyboardEvent<HTMLButtonElement>) => {
    onKeyDown?.(e);
    if (e.defaultPrevented || open) return;
    const k = e.key;
    if (k === 'ArrowDown' || k === 'ArrowUp' || k === 'Enter' || k === ' ') { e.preventDefault(); openAt(selected); }
    else if (k === 'Home') { e.preventDefault(); openAt(firstEnabled(options, 0, 1)); }
    else if (k === 'End') { e.preventDefault(); openAt(firstEnabled(options, options.length - 1, -1)); }
    else if (k.length === 1 && /\S/.test(k) && !e.ctrlKey && !e.metaKey && !e.altKey) {
      e.preventDefault();
      openAt(typeahead(options, selected, k, ta.current, Date.now()));
    }
  };

  const setRef = (el: HTMLButtonElement | null) => {
    trigger.current = el;
    if (typeof ref === 'function') ref(el);
    else if (ref) (ref as { current: HTMLButtonElement | null }).current = el;
  };

  return (
    <>
      <button
        {...a11y}
        {...rest}
        ref={setRef}
        type="button"
        role="combobox"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? listId : undefined}
        disabled={disabled}
        className={cx(s.trigger, className)}
        onClick={(e) => { onClick?.(e); if (!e.defaultPrevented) (open ? close(true) : openAt(selected)); }}
        onKeyDown={onTriggerKey}
      >
        <span className={s.value}>{current?.label ?? ''}</span>
        <Icon name="chevron-down" size={16} className={s.chevron} />
      </button>
      {open && createPortal(
        <div
          ref={list}
          id={listId}
          role="listbox"
          aria-labelledby={rest['aria-labelledby'] ?? a11y.id}
          tabIndex={-1}
          className={s.list}
          style={pos ? { top: pos.top, left: pos.left, inlineSize: pos.width, maxBlockSize: pos.maxHeight } : { top: -9999, left: -9999 }}
          onKeyDown={onListKey}
          data-loci-keep=""
        >
          {options.map((o, i) => (
            <div
              key={String(o.value)}
              id={`${listId}-${i}`}
              role="option"
              aria-selected={i === selected}
              aria-disabled={o.disabled || undefined}
              tabIndex={i === active ? 0 : -1}
              data-active={i === active || undefined}
              className={s.option}
              onPointerMove={() => { if (!o.disabled && i !== active) setActive(i); }}
              onClick={() => pick(i)}
            >
              <span className={s.optionLabel}>{o.label}</span>
              {i === selected && <Icon name="check" size={16} className={s.check} />}
            </div>
          ))}
        </div>,
        document.body,
      )}
    </>
  );
}
