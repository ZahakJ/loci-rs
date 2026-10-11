/**
 * Touch-only detection, decided once at first paint and kept in sync: keycaps never render on
 * phones and tablets without a mouse, whatever the CSS loading order (DESIGN §8).
 */
import { useSyncExternalStore } from 'react';

type Matcher = (query: string) => boolean;

/** Coarse primary pointer with no hover, or no fine pointer at all. */
export function isTouchOnly(matches: Matcher): boolean {
  if (!matches('(pointer: coarse)')) return false;
  return matches('(hover: none)') || !matches('(any-pointer: fine)');
}

const QUERIES = ['(pointer: coarse)', '(hover: none)', '(any-pointer: fine)'];
const browserMatch: Matcher = (q) => typeof window !== 'undefined' && typeof window.matchMedia === 'function' && window.matchMedia(q).matches;

let current = typeof window !== 'undefined' ? isTouchOnly(browserMatch) : false;
const subs = new Set<() => void>();

function apply(): void {
  if (typeof document !== 'undefined') document.documentElement.dataset.pointer = current ? 'touch' : 'fine';
}
apply();

if (typeof window !== 'undefined' && typeof window.matchMedia === 'function') {
  for (const q of QUERIES) {
    window.matchMedia(q).addEventListener?.('change', () => {
      const next = isTouchOnly(browserMatch);
      if (next === current) return;
      current = next;
      apply();
      subs.forEach((f) => f());
    });
  }
}

/** True on touch-only devices (no keyboard hints). */
export function touchOnly(): boolean { return current; }

export function useTouchOnly(): boolean {
  return useSyncExternalStore(
    (cb) => { subs.add(cb); return () => { subs.delete(cb); }; },
    () => current,
    () => false,
  );
}
