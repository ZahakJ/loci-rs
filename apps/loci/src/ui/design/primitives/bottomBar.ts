/**
 * Sticky bottom bars (study answer bar, phone tab bar, unsaved-changes bars) register here; the
 * tallest one sets `--bottom-bar-height` on <html> so floating layers (toasts) sit above it
 * instead of covering its buttons. A bar that is hidden (display: none) measures 0.
 */
import { useLayoutEffect, type RefObject } from 'react';

const bars = new Map<symbol, HTMLElement>();
let frame = 0;
let listening = false;

/** Space a bar takes from the bottom of the viewport: from its top edge to the viewport bottom. */
export function barSpace(rect: { top: number; height: number }, viewportHeight: number): number {
  if (rect.height <= 0) return 0;
  const space = viewportHeight - rect.top;
  // A bar scrolled up the page (not stuck to the bottom) does not cover the toast area.
  if (space <= 0 || space > rect.height + 160) return 0;
  return Math.round(space);
}

function measure(): void {
  frame = 0;
  if (typeof document === 'undefined') return;
  const vh = window.innerHeight;
  let max = 0;
  for (const el of bars.values()) {
    if (!el.isConnected) continue;
    max = Math.max(max, barSpace(el.getBoundingClientRect(), vh));
  }
  const html = document.documentElement;
  if (max > 0) html.style.setProperty('--bottom-bar-height', `${max}px`);
  else html.style.removeProperty('--bottom-bar-height');
}

function schedule(): void {
  if (frame || typeof window === 'undefined') return;
  frame = window.requestAnimationFrame(measure);
}

const ro = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(schedule) : null;

function listen(on: boolean): void {
  if (on === listening || typeof window === 'undefined') return;
  listening = on;
  const fn = on ? window.addEventListener : window.removeEventListener;
  fn('resize', schedule);
  fn('scroll', schedule, { capture: true, passive: true } as AddEventListenerOptions);
}

/** Registers `ref` as a bottom bar while `active`. */
export function useBottomBar(ref: RefObject<HTMLElement | null>, active = true): void {
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || !active) return;
    const id = Symbol('bottom-bar');
    bars.set(id, el);
    ro?.observe(el);
    listen(true);
    measure();
    // entrance animations move the bar for a moment; measure again once they settle
    const late = window.setTimeout(schedule, 400);
    return () => {
      window.clearTimeout(late);
      bars.delete(id);
      ro?.unobserve(el);
      if (bars.size === 0) listen(false);
      measure();
    };
  }, [ref, active]);
}
