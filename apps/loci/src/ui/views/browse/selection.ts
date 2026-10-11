/**
 * Browse table selection (pure reducer; tested). Rows are addressed by index into the ordered
 * id list of the current search; selection holds ids so it survives re-sorting.
 * - plain click / j k: the row becomes current and the only selection
 * - ctrl/cmd click: toggles the row, keeps the rest
 * - shift click / shift+j k: selects the range from the anchor to the row
 * - select all: every id of the search (api.searchIds)
 */
export type RowId = number;

export interface SelectionState {
  selected: ReadonlySet<RowId>;
  /** row the editor shows / keyboard focus is on */
  cursor: number | null;
  /** start of a shift range */
  anchor: number | null;
}

export type SelectionAction =
  | { type: 'click'; index: number; ids: readonly RowId[]; shift?: boolean; toggle?: boolean }
  | { type: 'move'; delta: number; ids: readonly RowId[]; extend?: boolean }
  | { type: 'selectAll'; ids: readonly RowId[] }
  | { type: 'set'; ids: readonly RowId[] }
  | { type: 'clear' }
  | { type: 'reset' }
  /** after a refetch: drop ids that are gone and clamp the cursor */
  | { type: 'prune'; ids: readonly RowId[] };

export const emptySelection: SelectionState = { selected: new Set(), cursor: null, anchor: null };

function range(ids: readonly RowId[], a: number, b: number): Set<RowId> {
  const out = new Set<RowId>();
  const lo = Math.max(0, Math.min(a, b)), hi = Math.min(ids.length - 1, Math.max(a, b));
  for (let i = lo; i <= hi; i++) out.add(ids[i]!);
  return out;
}

export function selectionReducer(state: SelectionState, action: SelectionAction): SelectionState {
  switch (action.type) {
    case 'click': {
      const { index, ids } = action;
      const id = ids[index];
      if (id === undefined) return state;
      if (action.shift) {
        const anchor = state.anchor ?? state.cursor ?? index;
        const sel = range(ids, anchor, index);
        if (action.toggle) for (const x of state.selected) sel.add(x);
        return { selected: sel, cursor: index, anchor };
      }
      if (action.toggle) {
        const sel = new Set(state.selected);
        if (sel.has(id)) sel.delete(id); else sel.add(id);
        return { selected: sel, cursor: index, anchor: index };
      }
      return { selected: new Set([id]), cursor: index, anchor: index };
    }
    case 'move': {
      const { ids } = action;
      if (ids.length === 0) return state;
      const from = state.cursor ?? (action.delta > 0 ? -1 : ids.length);
      const index = Math.max(0, Math.min(ids.length - 1, from + action.delta));
      if (action.extend) {
        const anchor = state.anchor ?? state.cursor ?? index;
        return { selected: range(ids, anchor, index), cursor: index, anchor };
      }
      return { selected: new Set([ids[index]!]), cursor: index, anchor: index };
    }
    case 'selectAll': return { ...state, selected: new Set(action.ids) };
    case 'set': return { ...state, selected: new Set(action.ids) };
    case 'clear': return { ...state, selected: new Set(), anchor: state.cursor };
    case 'reset': return emptySelection;
    case 'prune': {
      const valid = new Set(action.ids);
      const selected = new Set([...state.selected].filter((id) => valid.has(id)));
      const clamp = (i: number | null) => (i === null || action.ids.length === 0 ? null : Math.min(i, action.ids.length - 1));
      if (selected.size === state.selected.size && clamp(state.cursor) === state.cursor) return state;
      return { selected, cursor: clamp(state.cursor), anchor: clamp(state.anchor) };
    }
  }
}

/** "1,204 cards · 12 selected" pieces (numbers only; the view formats). */
export function selectionSummary(state: SelectionState): { count: number } {
  return { count: state.selected.size };
}
