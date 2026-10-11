/**
 * The UI's only door to collection data. Selects the real worker client, or the mock
 * (`?mock=1` or VITE_MOCK_API=1), exposes a queueing proxy `api`, event hooks and `useQuery`.
 */
import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore, type DependencyList } from 'react';
import type { CollectionApi, CollectionEvent } from '@core/api';
import type { IntegrityReport } from '@core/types';
import { QueryRunner, shouldRefetch, type QueryState, type Watch } from './hooks/queryRunner';

export type EventType = CollectionEvent['type'] | '*';
export interface ApiEvents { on(type: EventType, cb: (e: CollectionEvent) => void): () => void }
export interface ApiHandle { api: CollectionApi; events: ApiEvents }

export function isMockMode(): boolean {
  if (import.meta.env.VITE_MOCK_API === '1') return true;
  if (typeof window === 'undefined') return false;
  const p = new URLSearchParams(window.location.search);
  return p.get('mock') === '1' || p.has('mock') && p.get('mock') !== '0';
}

async function boot(): Promise<ApiHandle> {
  if (isMockMode()) {
    const { createMockApi } = await import('./dev/mockApi');
    const p = new URLSearchParams(window.location.search);
    return createMockApi({ empty: p.get('empty') === '1' });
  }
  const { createBridgeApi } = await import('../bridge');
  return createBridgeApi();
}

let handle: ApiHandle | null = null;
const pendingSubs = new Set<{ type: EventType; cb: (e: CollectionEvent) => void; off?: () => void }>();

/** Resolves with the ready API handle (after `open()`). */
export const handleReady: Promise<ApiHandle> = boot().then(async (h) => {
  handle = h;
  for (const sub of pendingSubs) sub.off = h.events.on(sub.type, sub.cb);
  return h;
});

/** Integrity report from opening the collection. */
export const openReport: Promise<IntegrityReport> = handleReady.then((h) => h.api.open());

/** Resolves with the API once the collection is open. */
export const ready: Promise<CollectionApi> = openReport.then(() => handle!.api);

/** Proxy that forwards every method to the ready instance, queueing calls until then. */
export const api: CollectionApi = new Proxy({} as CollectionApi, {
  get(_t, prop) {
    if (prop === 'then' || typeof prop === 'symbol') return undefined;
    return (...args: unknown[]) => {
      if (prop === 'open') return handleReady.then((h) => (h.api.open as (...a: unknown[]) => unknown)(...args));
      const result = ready.then((a) => {
        const fn = (a as unknown as Record<string, unknown>)[prop];
        if (typeof fn !== 'function') throw new Error(`CollectionApi.${String(prop)} is not implemented`);
        return (fn as (...a: unknown[]) => unknown).apply(a, args);
      });
      // Remember the rejection so the shell's global error toast can stay quiet: the caller owns it.
      result.catch((e: unknown) => noteCallError(e));
      return result;
    };
  },
});

// ── errors already delivered to a caller ──
const callErrors: Array<{ message: string; at: number }> = [];
function noteCallError(e: unknown): void {
  const now = Date.now();
  callErrors.push({ message: errorMessage(e), at: now });
  while (callErrors.length > 20 || (callErrors[0] && now - callErrors[0].at > 5000)) callErrors.shift();
}
/**
 * True when an 'error' event describes a failure that a caller already received as a rejected
 * promise (the worker reports both). The caller shows it inline or in its own toast, so the
 * shell does not repeat it. Errors with no caller (automatic backups) return false.
 */
export function isCallerError(message: string, now: number = Date.now()): boolean {
  return callErrors.some((c) => c.message === message && now - c.at < 5000);
}

/** Feature probe: resolves true when the collection implements `method` (e.g. a newer API method). */
export function apiHas(method: string): Promise<boolean> {
  return ready.then((a) => typeof (a as unknown as Record<string, unknown>)[method] === 'function', () => false);
}

/** Hook form of `apiHas`: false until known. */
export function useApiHas(method: string): boolean {
  const [has, setHas] = useState(false);
  useEffect(() => {
    let alive = true;
    void apiHas(method).then((v) => { if (alive) setHas(v); });
    return () => { alive = false; };
  }, [method]);
  return has;
}

/** Subscribes to collection events (works before the API is ready). */
export function onCollectionEvent(type: EventType, cb: (e: CollectionEvent) => void): () => void {
  if (handle) return handle.events.on(type, cb);
  const sub: { type: EventType; cb: (e: CollectionEvent) => void; off?: () => void } = { type, cb };
  pendingSubs.add(sub);
  return () => { pendingSubs.delete(sub); sub.off?.(); };
}

export function useApi(): CollectionApi { return api; }

export function useCollectionEvent(type: EventType, cb: (e: CollectionEvent) => void): void {
  const ref = useRef(cb);
  useLayoutEffect(() => { ref.current = cb; });
  useEffect(() => onCollectionEvent(type, (e) => ref.current(e)), [type]);
}

export interface QueryResult<T> extends QueryState<T> {
  refetch: () => Promise<void>;
  setData: (update: T | ((prev: T | undefined) => T)) => void;
}

/**
 * Small data hook: runs `fn` when `deps` change, and again (coalesced, keeping old data)
 * when a 'changed' event touches one of `watch` (default: any change).
 */
export function useQuery<T>(fn: (api: CollectionApi) => Promise<T>, deps: DependencyList, watch: Watch = 'all'): QueryResult<T> {
  const fnRef = useRef(fn);
  useLayoutEffect(() => { fnRef.current = fn; });
  const [runner] = useState(() => new QueryRunner<T>(() => fnRef.current(api)));
  const state = useSyncExternalStore(runner.subscribe, runner.getState, runner.getState);
  useEffect(() => {
    void runner.run();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  const watchKey = typeof watch === 'string' ? watch : watch.join(',');
  useEffect(() => {
    const w: Watch = watchKey === 'all' || watchKey === 'none' ? watchKey : (watchKey.split(',') as Watch);
    return onCollectionEvent('changed', (e) => { if (shouldRefetch(e, w)) runner.schedule(); });
  }, [runner, watchKey]);
  useEffect(() => () => runner.dispose(), [runner]);
  return { ...state, refetch: runner.run, setData: runner.setData };
}

/**
 * Saves bytes as a file: through the Rust shell (native save dialog on desktop, the app's downloads
 * folder on Android) when running in Tauri, otherwise a browser download. Errors are toasted here.
 */
export function downloadBytes(bytes: Uint8Array, filename: string, mime = 'application/octet-stream'): void {
  void (async () => {
    const { activeTransport } = await import('../bridge');
    const transport = activeTransport();
    if (!transport) {
      const blob = new Blob([bytes as BlobPart], { type: mime });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = filename;
      document.body.appendChild(a);
      a.click();
      a.remove();
      setTimeout(() => URL.revokeObjectURL(url), 2000);
      return;
    }
    const where = await transport.saveFile(bytes, filename, mime);
    if (where && transport.kind === 'tauri') {
      const { toast } = await import('./design/primitives/Toast');
      toast({ message: where, icon: 'download', tone: 'success', id: `saved:${where}` });
    }
  })().catch(async (e: unknown) => {
    const { toast } = await import('./design/primitives/Toast');
    toast({ message: errorMessage(e), tone: 'danger' });
  });
}

/** User-facing message for an error thrown by the API. */
export function errorMessage(e: unknown): string {
  if (e && typeof e === 'object' && 'message' in e && typeof (e as { message: unknown }).message === 'string') return (e as { message: string }).message;
  return String(e);
}
export function errorCode(e: unknown): string | undefined {
  return e && typeof e === 'object' && 'code' in e ? String((e as { code: unknown }).code) : undefined;
}

/** Collection methods newer than some builds; probe with `apiHas` / `useApiHas` before offering them. */
interface NewerApi {
  /** Raises today's new/review limits for a deck and its subdecks by the given amounts (undoable). */
  extendTodayLimits(deckId: number, extraNew: number, extraReview: number): Promise<unknown>;
}
export const EXTEND_TODAY = 'extendTodayLimits';
/**
 * Raises today's limits for a deck, or (null) for every top-level deck: a session over all decks
 * is capped by each deck's own limits, so each one gets the extra allowance.
 */
export async function extendTodayLimits(deckId: number | null, extraNew: number, extraReview: number): Promise<void> {
  const ext = api as unknown as NewerApi;
  if (deckId !== null) { await ext.extendTodayLimits(deckId, extraNew, extraReview); return; }
  const tree = await api.deckTree();
  for (const child of tree.children) await ext.extendTodayLimits(child.deck.id, extraNew, extraReview);
}
