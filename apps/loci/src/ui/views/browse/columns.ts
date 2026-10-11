/** Browse columns: labels, widths, defaults per mode, and the remembered layout (localStorage). */
import type { BrowseColumn, BrowseMode } from '@core/types';

export const ALL_COLUMNS: BrowseColumn[] = [
  'sortField', 'question', 'answer', 'deck', 'notetype', 'template', 'tags', 'due', 'interval', 'ease',
  'difficulty', 'stability', 'retrievability', 'reps', 'lapses', 'created', 'modified', 'cardCount',
];

/** Columns only meaningful per card (hidden in notes mode). */
export const CARD_ONLY: ReadonlySet<BrowseColumn> = new Set(['template', 'due', 'interval', 'ease', 'difficulty', 'stability', 'retrievability', 'reps', 'lapses']);
/** Columns only meaningful per note. */
export const NOTE_ONLY: ReadonlySet<BrowseColumn> = new Set(['cardCount']);

/** Track sizes: the first text column takes the slack. */
export const WIDTH: Record<BrowseColumn, string> = {
  sortField: 'minmax(10rem, 3fr)', question: 'minmax(10rem, 2fr)', answer: 'minmax(10rem, 2fr)',
  deck: 'minmax(7rem, 1.2fr)', notetype: 'minmax(7rem, 1fr)', template: '6.5rem', tags: 'minmax(7rem, 1fr)',
  due: '6.5rem', interval: '5.5rem', ease: '4.5rem', difficulty: '5.5rem', stability: '5.5rem', retrievability: '6rem',
  reps: '4.5rem', lapses: '4.5rem', created: '6.5rem', modified: '6.5rem', cardCount: '4rem',
};
export const NUMERIC: ReadonlySet<BrowseColumn> = new Set(['interval', 'ease', 'difficulty', 'stability', 'retrievability', 'reps', 'lapses', 'cardCount']);

export const DEFAULT_COLUMNS: Record<BrowseMode, BrowseColumn[]> = {
  cards: ['sortField', 'template', 'due', 'deck'],
  notes: ['sortField', 'notetype', 'cardCount', 'tags'],
};

export interface BrowseLayout {
  mode: BrowseMode;
  columns: Record<BrowseMode, BrowseColumn[]>;
  sort: Record<BrowseMode, { column: BrowseColumn; descending: boolean }>;
  sidebar: boolean;
}

const KEY = 'loci.browse.layout.v2';
export const DEFAULT_LAYOUT: BrowseLayout = {
  mode: 'cards',
  columns: DEFAULT_COLUMNS,
  sort: { cards: { column: 'sortField', descending: false }, notes: { column: 'sortField', descending: false } },
  sidebar: true,
};

export function readLayout(): BrowseLayout {
  try {
    const raw = window.localStorage.getItem(KEY);
    if (!raw) return DEFAULT_LAYOUT;
    const v = JSON.parse(raw) as Partial<BrowseLayout>;
    const valid = (cols: unknown, mode: BrowseMode): BrowseColumn[] =>
      Array.isArray(cols) && cols.length ? cols.filter((c): c is BrowseColumn => ALL_COLUMNS.includes(c as BrowseColumn)) : DEFAULT_COLUMNS[mode];
    return {
      mode: v.mode === 'notes' ? 'notes' : 'cards',
      columns: { cards: valid(v.columns?.cards, 'cards'), notes: valid(v.columns?.notes, 'notes') },
      sort: { ...DEFAULT_LAYOUT.sort, ...(v.sort ?? {}) },
      sidebar: v.sidebar !== false,
    };
  } catch { return DEFAULT_LAYOUT; }
}

export function writeLayout(l: BrowseLayout): void {
  try { window.localStorage.setItem(KEY, JSON.stringify(l)); } catch { /* unavailable */ }
}

/** Toggles a column, keeping at least one and the canonical order. */
export function toggleColumn(cols: BrowseColumn[], c: BrowseColumn): BrowseColumn[] {
  if (cols.includes(c)) return cols.length > 1 ? cols.filter((x) => x !== c) : cols;
  return ALL_COLUMNS.filter((x) => x === c || cols.includes(x));
}
