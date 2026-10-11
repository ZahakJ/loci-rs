import { cloneElement, useEffect, useId, useLayoutEffect, useRef, useState, type ReactElement, type ReactNode, type SyntheticEvent } from 'react';
import { createPortal } from 'react-dom';
import s from './Tooltip.module.css';

export interface TooltipProps {
  content: ReactNode;
  /** a single focusable element */
  children: ReactElement<Record<string, unknown>>;
  placement?: 'top' | 'bottom';
  delay?: number;
}

type Handler = ((e: SyntheticEvent) => void) | undefined;

/**
 * Hover/focus tooltip; on touch a long-press (500ms) shows it for 1.5s.
 * Adds aria-describedby to the child. Esc hides it.
 */
export function Tooltip({ content, children, placement = 'top', delay = 450 }: TooltipProps) {
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<{ top: number; left: number } | null>(null);
  const anchor = useRef<HTMLElement | null>(null);
  const tip = useRef<HTMLDivElement>(null);
  const timer = useRef<number | undefined>(undefined);
  const id = useId();

  const clear = () => window.clearTimeout(timer.current);
  const show = (after: number) => { clear(); timer.current = window.setTimeout(() => setOpen(true), after); };
  const hide = () => { clear(); setOpen(false); };

  useEffect(() => () => clear(), []);
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') hide(); };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [open]);

  useLayoutEffect(() => {
    if (!open || !anchor.current || !tip.current) { setPos(null); return; }
    const r = anchor.current.getBoundingClientRect();
    const t = tip.current.getBoundingClientRect();
    let top = placement === 'top' ? r.top - t.height - 8 : r.bottom + 8;
    if (top < 8) top = r.bottom + 8;
    if (top + t.height > window.innerHeight - 8) top = r.top - t.height - 8;
    const left = Math.max(8, Math.min(r.left + r.width / 2 - t.width / 2, window.innerWidth - t.width - 8));
    setPos({ top, left });
  }, [open, placement]);

  const p = children.props;
  const chain = (name: string, fn: (e: SyntheticEvent) => void) => (e: SyntheticEvent) => { (p[name] as Handler)?.(e); fn(e); };
  const child = cloneElement(children, {
    ref: (el: HTMLElement | null) => {
      anchor.current = el;
      const r = (children as unknown as { props: { ref?: unknown } }).props.ref;
      if (typeof r === 'function') r(el);
      else if (r && typeof r === 'object') (r as { current: unknown }).current = el;
    },
    'aria-describedby': open ? id : (p['aria-describedby'] as string | undefined),
    onPointerEnter: chain('onPointerEnter', (e) => { if ((e.nativeEvent as PointerEvent).pointerType === 'mouse') show(delay); }),
    onPointerLeave: chain('onPointerLeave', hide),
    onFocus: chain('onFocus', (e) => { if ((e.target as HTMLElement).matches(':focus-visible')) show(0); }),
    onBlur: chain('onBlur', hide),
    onPointerDown: chain('onPointerDown', (e) => {
      if ((e.nativeEvent as PointerEvent).pointerType !== 'mouse') {
        clear();
        timer.current = window.setTimeout(() => { setOpen(true); timer.current = window.setTimeout(() => setOpen(false), 1500); }, 500);
      }
    }),
    onPointerUp: chain('onPointerUp', (e) => { if ((e.nativeEvent as PointerEvent).pointerType !== 'mouse' && !open) clear(); }),
    onContextMenu: chain('onContextMenu', (e) => { if (open) e.preventDefault(); }),
  });

  return (
    <>
      {child}
      {open && createPortal(
        <div ref={tip} id={id} role="tooltip" className={s.tip} style={pos ?? { top: -9999, left: -9999 }} data-loci-keep="">
          {content}
        </div>,
        document.body,
      )}
    </>
  );
}
