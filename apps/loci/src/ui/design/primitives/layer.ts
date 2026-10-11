/**
 * Modal layer stack shared by Sheet and Dialog: focus trap, Escape to close,
 * focus return, and `inert` on everything behind the top layer.
 */
import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from 'react';

interface LayerEntry { node: HTMLElement; onEscape: () => void; inerted: Element[] }
const stack: LayerEntry[] = [];
const listeners = new Set<() => void>();

/** True while any Sheet/Dialog is open (global shortcuts pause). */
export function isModalOpen(): boolean { return stack.length > 0; }
export function subscribeModal(cb: () => void): () => void { listeners.add(cb); return () => listeners.delete(cb); }
const emit = () => listeners.forEach((l) => l());

const FOCUSABLE = [
  'a[href]', 'area[href]', 'button:not([disabled])', 'input:not([disabled]):not([type="hidden"])',
  'select:not([disabled])', 'textarea:not([disabled])', 'iframe', 'audio[controls]', 'video[controls]',
  '[contenteditable]:not([contenteditable="false"])', '[tabindex]:not([tabindex="-1"])',
].join(',');

export function focusables(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (el) => !el.hasAttribute('inert') && el.getClientRects().length > 0 && !el.closest('[inert]'),
  );
}

function onKeyDown(e: KeyboardEvent) {
  const top = stack[stack.length - 1];
  if (!top) return;
  if (e.key === 'Escape' && !e.defaultPrevented) {
    e.preventDefault();
    e.stopPropagation();
    top.onEscape();
    return;
  }
  if (e.key === 'Tab') {
    const items = focusables(top.node);
    if (items.length === 0) { e.preventDefault(); top.node.focus(); return; }
    const first = items[0]!;
    const last = items[items.length - 1]!;
    const active = document.activeElement;
    if (e.shiftKey && (active === first || !top.node.contains(active))) { e.preventDefault(); last.focus(); }
    else if (!e.shiftKey && (active === last || !top.node.contains(active))) { e.preventDefault(); first.focus(); }
  }
}

function pushLayer(entry: LayerEntry) {
  if (stack.length === 0) document.addEventListener('keydown', onKeyDown, true);
  for (const child of Array.from(document.body.children)) {
    if (child === entry.node || child.contains(entry.node)) continue;
    if (child.hasAttribute('data-loci-keep') || child.tagName === 'SCRIPT') continue;
    if (child.hasAttribute('inert')) continue;
    child.setAttribute('inert', '');
    entry.inerted.push(child);
  }
  stack.push(entry);
  emit();
}

function popLayer(entry: LayerEntry) {
  const i = stack.indexOf(entry);
  if (i >= 0) stack.splice(i, 1);
  for (const el of entry.inerted) el.removeAttribute('inert');
  entry.inerted = [];
  if (stack.length === 0) document.removeEventListener('keydown', onKeyDown, true);
  emit();
}

/**
 * Activates modal behaviour for `ref` while `active` is true.
 * `initialFocus` may point at an element; otherwise `[data-autofocus]`, then the first focusable, then the panel.
 */
export function useModalLayer(
  ref: RefObject<HTMLElement | null>,
  active: boolean,
  onEscape: () => void,
  initialFocus?: RefObject<HTMLElement | null>,
) {
  const escRef = useRef(onEscape);
  useLayoutEffect(() => { escRef.current = onEscape; });
  useEffect(() => {
    const node = ref.current;
    if (!active || !node) return;
    const previous = document.activeElement as HTMLElement | null;
    const entry: LayerEntry = { node, onEscape: () => escRef.current(), inerted: [] };
    // Portal root is the element appended to body; find it.
    let host: HTMLElement = node;
    while (host.parentElement && host.parentElement !== document.body) host = host.parentElement;
    entry.node = host;
    pushLayer(entry);
    const target = initialFocus?.current ?? node.querySelector<HTMLElement>('[data-autofocus]') ?? focusables(node)[0] ?? node;
    requestAnimationFrame(() => target.focus({ preventScroll: true }));
    return () => {
      popLayer(entry);
      if (previous && previous.isConnected) requestAnimationFrame(() => previous.focus({ preventScroll: true }));
    };
  }, [active, ref, initialFocus]);
}

function readMs(name: string): number {
  if (typeof window === 'undefined') return 0;
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  const n = parseFloat(v);
  return Number.isFinite(n) ? (v.endsWith('ms') ? n : n * 1000) : 0;
}

/** Keeps a component mounted while it animates out. `state` drives CSS data attributes. */
export function usePresence(open: boolean, token = '--d-base'): { mounted: boolean; state: 'open' | 'closed' } {
  const [mounted, setMounted] = useState(open);
  const [state, setState] = useState<'open' | 'closed'>(open ? 'open' : 'closed');
  useEffect(() => {
    if (open) {
      setMounted(true);
      const id = requestAnimationFrame(() => setState('open'));
      return () => cancelAnimationFrame(id);
    }
    setState('closed');
    const ms = readMs(token);
    if (ms <= 0) { setMounted(false); return; }
    const id = window.setTimeout(() => setMounted(false), ms);
    return () => window.clearTimeout(id);
  }, [open, token]);
  return { mounted, state: open ? 'open' : state };
}

/** Matches a CSS media query and updates on change. */
export function useMedia(query: string): boolean {
  const [match, setMatch] = useState(() => typeof window !== 'undefined' && window.matchMedia(query).matches);
  useEffect(() => {
    const mq = window.matchMedia(query);
    const on = () => setMatch(mq.matches);
    on();
    mq.addEventListener('change', on);
    return () => mq.removeEventListener('change', on);
  }, [query]);
  return match;
}

export function cx(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(' ');
}
