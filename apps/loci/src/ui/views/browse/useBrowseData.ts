/**
 * Paged loading for the virtual table: the first page brings the total and the ordered id list;
 * further pages load as rows scroll into view. A collection change refetches the visible pages
 * without resetting scroll or selection.
 */
import { useCallback, useEffect, useRef, useState } from 'react';
import type { BrowseColumn, BrowseMode, BrowseRow } from '@core/types';
import { api, errorMessage, onCollectionEvent } from '@ui/api';

const PAGE = 100;

export interface BrowseData {
  total: number;
  ids: Array<number>;
  row: (index: number) => BrowseRow | undefined;
  ensure: (start: number, end: number) => void;
  loading: boolean;
  fetching: boolean;
  error: string | null;
  /** bumps when rows change (forces a table re-render) */
  version: number;
  refetch: () => void;
}

export function useBrowseData(query: string, mode: BrowseMode, sort: { column: BrowseColumn; descending: boolean }): BrowseData {
  const [state, setState] = useState({ total: 0, ids: [] as number[], loading: true, fetching: true, error: null as string | null, version: 0 });
  const pages = useRef(new Map<number, BrowseRow[]>());
  const pending = useRef(new Set<number>());
  const seq = useRef(0);
  const visible = useRef<[number, number]>([0, 0]);

  const fetchPage = useCallback(async (page: number, run: number) => {
    if (pending.current.has(page)) return;
    pending.current.add(page);
    try {
      const res = await api.browse({ query, mode, sort, offset: page * PAGE, limit: PAGE });
      if (run !== seq.current) return;
      pages.current.set(page, res.rows);
      setState((st) => ({ ...st, total: res.total, ids: res.ids as number[], loading: false, fetching: false, error: null, version: st.version + 1 }));
    } catch (e) {
      if (run !== seq.current) return;
      setState((st) => ({ ...st, loading: false, fetching: false, error: errorMessage(e) }));
    } finally { pending.current.delete(page); }
  }, [query, mode, sort]);

  const load = useCallback((keep: boolean) => {
    const run = ++seq.current;
    pending.current.clear();
    const [a, b] = visible.current;
    const first = Math.floor(a / PAGE), last = Math.floor(b / PAGE);
    if (!keep) pages.current = new Map();
    setState((st) => ({ ...st, fetching: true, loading: keep ? st.loading : true, error: null }));
    for (let p = keep ? first : 0; p <= (keep ? last : 0); p++) void fetchPage(p, run).then(() => {
      if (keep) for (const k of [...pages.current.keys()]) if (k < first || k > last) pages.current.delete(k);
    });
  }, [fetchPage]);

  useEffect(() => { visible.current = [0, 0]; load(false); }, [load]);
  useEffect(() => onCollectionEvent('changed', (e) => {
    if (e.type === 'changed' && e.what.some((w) => w === 'cards' || w === 'notes' || w === 'decks' || w === 'notetypes' || w === 'tags')) load(true);
  }), [load]);

  const ensure = useCallback((start: number, end: number) => {
    visible.current = [start, end];
    const run = seq.current;
    for (let p = Math.floor(start / PAGE); p <= Math.floor(end / PAGE); p++) {
      if (!pages.current.has(p)) void fetchPage(p, run);
    }
  }, [fetchPage]);

  const row = useCallback((i: number) => pages.current.get(Math.floor(i / PAGE))?.[i % PAGE], []);

  return { ...state, row, ensure, refetch: () => load(true) };
}
