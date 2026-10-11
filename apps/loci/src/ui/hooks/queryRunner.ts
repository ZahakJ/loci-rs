/**
 * Framework-free query state machine behind useQuery (tested in node).
 * - latest request wins (stale responses are dropped)
 * - refetches keep the previous data (no skeleton flash); `loading` is only true before the first result
 * - change events are coalesced into one refetch
 */
import type { CollectionEvent } from '@core/api';

export type ChangeKey = Extract<CollectionEvent, { type: 'changed' }>['what'][number];
export type Watch = readonly ChangeKey[] | 'all' | 'none';

export function shouldRefetch(e: CollectionEvent, watch: Watch): boolean {
  if (e.type !== 'changed' || watch === 'none') return false;
  if (watch === 'all') return true;
  return e.what.some((w) => watch.includes(w));
}

export interface QueryState<T> {
  data: T | undefined;
  error: unknown;
  /** true until the first result (or error) arrives */
  loading: boolean;
  /** true while any request is in flight */
  fetching: boolean;
}

export class QueryRunner<T> {
  private state: QueryState<T> = { data: undefined, error: undefined, loading: true, fetching: false };
  private listeners = new Set<() => void>();
  private seq = 0;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private fn: () => Promise<T>;

  constructor(fn: () => Promise<T>, private coalesceMs = 30) { this.fn = fn; }

  setFn(fn: () => Promise<T>) { this.fn = fn; }
  getState = (): QueryState<T> => this.state;
  subscribe = (cb: () => void): (() => void) => { this.listeners.add(cb); return () => this.listeners.delete(cb); };

  private set(patch: Partial<QueryState<T>>) {
    this.state = { ...this.state, ...patch };
    this.listeners.forEach((l) => l());
  }

  /** Runs the query now; resolves when this run settles (even if superseded). */
  run = async (): Promise<void> => {
    const id = ++this.seq;
    this.set({ fetching: true });
    try {
      const data = await this.fn();
      if (id !== this.seq) return;
      this.set({ data, error: undefined, loading: false, fetching: false });
    } catch (error) {
      if (id !== this.seq) return;
      this.set({ error, loading: false, fetching: false });
    }
  };

  /** Schedules a coalesced refetch (used for change events). */
  schedule(): void {
    if (this.timer !== undefined) clearTimeout(this.timer);
    this.timer = setTimeout(() => { this.timer = undefined; void this.run(); }, this.coalesceMs);
  }

  /** Local optimistic update. */
  setData = (update: T | ((prev: T | undefined) => T)): void => {
    const data = typeof update === 'function' ? (update as (p: T | undefined) => T)(this.state.data) : update;
    this.set({ data });
  };

  /** Invalidates in-flight requests and timers. */
  dispose(): void {
    this.seq++;
    if (this.timer !== undefined) clearTimeout(this.timer);
  }
}
