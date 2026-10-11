import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { Icon, type IconName } from '../icons';
import { Kbd } from './Kbd';
import { cx } from './layer';
import s from './Menu.module.css';

export type MenuEntry =
  | { id: string; label: string; icon?: IconName; onSelect: () => void; danger?: boolean; disabled?: boolean; kbd?: string }
  | { separator: true; id: string };

export interface MenuTriggerProps {
  ref: (el: HTMLButtonElement | null) => void;
  onClick: () => void;
  onKeyDown: (e: KeyboardEvent<HTMLButtonElement>) => void;
  'aria-haspopup': 'menu';
  'aria-expanded': boolean;
  'aria-controls': string | undefined;
}

export interface MenuProps {
  /** accessible name of the menu */
  label: string;
  items: MenuEntry[];
  /** renders the trigger; spread the props onto a button */
  trigger: (props: MenuTriggerProps) => ReactNode;
  align?: 'start' | 'end';
  className?: string;
  onOpenChange?: (open: boolean) => void;
}

type Action = Extract<MenuEntry, { onSelect: () => void }>;
const isAction = (e: MenuEntry): e is Action => !('separator' in e);

/** Overflow menu: roving focus, arrow keys, Home/End, typeahead, Esc returns focus to the trigger. */
export function Menu({ label, items, trigger, align = 'end', className, onOpenChange }: MenuProps) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(-1);
  const [pos, setPos] = useState<{ top: number; left: number; maxHeight: number } | null>(null);
  const triggerEl = useRef<HTMLButtonElement | null>(null);
  const menuEl = useRef<HTMLDivElement>(null);
  const typeahead = useRef({ text: '', at: 0 });
  const menuId = useId();
  const actions = items.filter(isAction);

  const setOpenState = useCallback((v: boolean, focusTrigger = false) => {
    setOpen(v);
    onOpenChange?.(v);
    if (!v && focusTrigger) triggerEl.current?.focus();
  }, [onOpenChange]);

  const openAt = (index: number) => {
    setActive(index);
    setOpenState(true);
  };

  useLayoutEffect(() => {
    if (!open || !triggerEl.current || !menuEl.current) return;
    const r = triggerEl.current.getBoundingClientRect();
    const m = menuEl.current.getBoundingClientRect();
    const vw = window.innerWidth, vh = window.innerHeight;
    const rtl = document.documentElement.dir === 'rtl';
    const alignEnd = (align === 'end') !== rtl;
    let left = alignEnd ? r.right - m.width : r.left;
    left = Math.max(8, Math.min(left, vw - m.width - 8));
    const below = vh - r.bottom - 8;
    const above = r.top - 8;
    const placeAbove = below < Math.min(m.height, 240) && above > below;
    const maxHeight = Math.max(160, (placeAbove ? above : below) - 4);
    const top = placeAbove ? Math.max(8, r.top - Math.min(m.height, maxHeight) - 4) : r.bottom + 4;
    setPos({ top, left, maxHeight });
  }, [open, align]);

  useEffect(() => {
    if (!open) { setPos(null); return; }
    const onDown = (e: PointerEvent) => {
      const t = e.target as Node;
      if (menuEl.current?.contains(t) || triggerEl.current?.contains(t)) return;
      setOpenState(false);
    };
    const onScroll = (e: Event) => { if (!menuEl.current?.contains(e.target as Node)) setOpenState(false); };
    document.addEventListener('pointerdown', onDown, true);
    window.addEventListener('resize', onScroll);
    window.addEventListener('scroll', onScroll, true);
    return () => {
      document.removeEventListener('pointerdown', onDown, true);
      window.removeEventListener('resize', onScroll);
      window.removeEventListener('scroll', onScroll, true);
    };
  }, [open, setOpenState]);

  const move = (from: number, dir: 1 | -1) => {
    const n = actions.length;
    for (let i = 1; i <= n; i++) {
      const idx = (from + dir * i + n * 2) % n;
      if (!actions[idx]?.disabled) return idx;
    }
    return from;
  };

  const select = (a: Action) => {
    if (a.disabled) return;
    setOpenState(false, true);
    // let focus return before running (dialogs opened by the action take focus next)
    requestAnimationFrame(() => a.onSelect());
  };

  const onMenuKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const k = e.key;
    if (k === 'ArrowDown') { e.preventDefault(); setActive((i) => move(i, 1)); }
    else if (k === 'ArrowUp') { e.preventDefault(); setActive((i) => move(i < 0 ? 0 : i, -1)); }
    else if (k === 'Home') { e.preventDefault(); setActive(move(-1, 1)); }
    else if (k === 'End') { e.preventDefault(); setActive(move(0, -1)); }
    else if (k === 'Escape') { e.preventDefault(); e.stopPropagation(); setOpenState(false, true); }
    else if (k === 'Tab') { setOpenState(false); }
    else if (k === 'Enter' || k === ' ') {
      e.preventDefault();
      const a = actions[active];
      if (a) select(a);
    } else if (k.length === 1 && /\S/.test(k) && !e.ctrlKey && !e.metaKey && !e.altKey) {
      const now = Date.now();
      const t = typeahead.current;
      t.text = now - t.at > 600 ? k.toLowerCase() : t.text + k.toLowerCase();
      t.at = now;
      const start = t.text.length === 1 ? active + 1 : active;
      for (let i = 0; i < actions.length; i++) {
        const idx = (start + i) % actions.length;
        const a = actions[idx]!;
        if (!a.disabled && a.label.toLowerCase().startsWith(t.text)) { setActive(idx); break; }
      }
    }
  };

  const onTriggerKey = (e: KeyboardEvent<HTMLButtonElement>) => {
    if (e.key === 'ArrowDown' || e.key === 'Enter' || e.key === ' ') { e.preventDefault(); openAt(move(-1, 1)); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); openAt(move(0, -1)); }
  };

  // Roving focus: the active item holds DOM focus; with none active the menu itself does.
  useEffect(() => {
    if (!open || !pos) return;
    const el = active >= 0 ? document.getElementById(`${menuId}-${active}`) : menuEl.current;
    el?.focus({ preventScroll: false });
  }, [open, pos, active, menuId]);
  let actionIndex = -1;
  return (
    <>
      {trigger({
        ref: (el) => { triggerEl.current = el; },
        onClick: () => (open ? setOpenState(false) : openAt(-1)),
        onKeyDown: onTriggerKey,
        'aria-haspopup': 'menu',
        'aria-expanded': open,
        'aria-controls': open ? menuId : undefined,
      })}
      {open && createPortal(
        <div
          ref={menuEl}
          id={menuId}
          role="menu"
          aria-label={label}
          tabIndex={-1}
          className={cx(s.menu, className)}
          style={pos ? { top: pos.top, left: pos.left, maxHeight: pos.maxHeight } : { top: -9999, left: -9999 }}
          onKeyDown={onMenuKey}
          data-loci-keep=""
        >
          {items.map((it) => {
            if (!isAction(it)) return <div key={it.id} role="separator" className={s.separator} />;
            actionIndex++;
            const idx = actionIndex;
            return (
              <button
                key={it.id}
                id={`${menuId}-${idx}`}
                type="button"
                role="menuitem"
                tabIndex={idx === active ? 0 : -1}
                aria-disabled={it.disabled || undefined}
                data-active={idx === active}
                className={cx(s.item, it.danger && s.danger)}
                onPointerMove={() => !it.disabled && setActive(idx)}
                onClick={() => select(it)}
              >
                {it.icon && <Icon name={it.icon} size={18} />}
                <span className={s.label}>{it.label}</span>
                {it.kbd && <Kbd>{it.kbd}</Kbd>}
              </button>
            );
          })}
        </div>,
        document.body,
      )}
    </>
  );
}
