import { cloneElement, useEffect, useId, useLayoutEffect, useRef, useState, type ReactElement, type ReactNode, type SyntheticEvent } from 'react';
import { createPortal } from 'react-dom';
import { cx } from './layer';
import { floorY } from './bottomBar';
import s from './Tooltip.module.css';

export interface TooltipProps {
  content: ReactNode;
  /** a single element (focusable for keyboard users); a disabled button is wrapped in a span */
  children: ReactElement<Record<string, unknown>>;
  placement?: 'top' | 'bottom';
  delay?: number;
  /** class for the wrapping span used around disabled controls */
  className?: string;
}

type Handler = ((e: SyntheticEvent) => void) | undefined;

/**
 * Hover/focus tooltip; on touch a long-press (500ms) shows it for 1.5s.
 * Adds aria-describedby to the child. Esc hides it. Replaces every `title` attribute in the UI:
 * OS tooltips are unstyled, slow, and never appear on touch.
 */
export function Tooltip({ content, children, placement = 'top', delay = 450, className }: TooltipProps) {
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
    const floor = floorY(); // never over the phone tab bar or the study answer bar
    let top = placement === 'top' ? r.top - t.height - 8 : r.bottom + 8;
    if (top < 8) top = r.bottom + 8;
    if (top + t.height > floor) top = r.top - t.height - 8;
    top = Math.max(8, top);
    const left = Math.max(8, Math.min(r.left + r.width / 2 - t.width / 2, window.innerWidth - t.width - 8));
    setPos({ top, left });
  }, [open, placement]);

  // A disabled button fires no pointer events: hang the handlers on a wrapping span instead.
  const wrap = children.props.disabled === true;
  const p = wrap ? {} as Record<string, unknown> : children.props;
  const chain = (name: string, fn: (e: SyntheticEvent) => void) => (e: SyntheticEvent) => { (p[name] as Handler)?.(e); fn(e); };
  const handlers = {
    'aria-describedby': wrap ? undefined : open ? id : (p['aria-describedby'] as string | undefined),
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
  };
  const setAnchor = (el: HTMLElement | null) => { anchor.current = el; };
  const child = wrap
    ? <span ref={setAnchor} className={cx(s.anchor, className)} {...handlers}>{children}</span>
    : cloneElement(children, {
      ...handlers,
      ref: (el: HTMLElement | null) => {
        setAnchor(el);
        const r = (children as unknown as { props: { ref?: unknown } }).props.ref;
        if (typeof r === 'function') r(el);
        else if (r && typeof r === 'object') (r as { current: unknown }).current = el;
      },
    });

  if (content === null || content === undefined || content === false || content === '') return children;
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
