/**
 * Cloze one-by-one reveal (DESIGN §8): on the question, each Space reveals the next active cloze;
 * once every cloze is shown, the next Space shows the answer. Pure reducer (tested).
 */
export interface OneByOneState { total: number; revealed: number }
export type OneByOneAction = { type: 'reset'; total: number } | { type: 'advance' } | { type: 'reveal-all' } | { type: 'reveal'; index: number };

export function oneByOneInit(total: number): OneByOneState {
  return { total: Math.max(0, Math.floor(total)), revealed: 0 };
}

export function oneByOneReducer(state: OneByOneState, action: OneByOneAction): OneByOneState {
  switch (action.type) {
    case 'reset': return oneByOneInit(action.total);
    case 'advance': return state.revealed < state.total ? { ...state, revealed: state.revealed + 1 } : state;
    case 'reveal-all': return state.revealed === state.total ? state : { ...state, revealed: state.total };
    // Clicking a hidden cloze reveals everything up to and including it (order is kept).
    case 'reveal': return action.index + 1 > state.revealed ? { ...state, revealed: Math.min(state.total, action.index + 1) } : state;
  }
}

/** True while Space should reveal another cloze rather than show the answer. */
export function oneByOnePending(state: OneByOneState): boolean {
  return state.total > 0 && state.revealed < state.total;
}
