/**
 * Keyboard model shared by the listbox primitives (Select): pure functions, so the behaviour is
 * unit-tested without a DOM. Disabled options are skipped; movement wraps like a native select's
 * popup does not, so ArrowDown on the last option stays there.
 */

export interface NavOption { label: string; disabled?: boolean }

/** First enabled index from `from` (inclusive) moving by `dir`, or -1. */
export function firstEnabled(options: NavOption[], from: number, dir: 1 | -1): number {
  for (let i = from; i >= 0 && i < options.length; i += dir) if (!options[i]?.disabled) return i;
  return -1;
}

/** The next enabled index after `from` in `dir`; stays at `from` at either end. */
export function step(options: NavOption[], from: number, dir: 1 | -1): number {
  const i = firstEnabled(options, from + dir, dir);
  return i < 0 ? from : i;
}

export interface Typeahead { text: string; at: number }

/**
 * Typeahead: keys typed within 600 ms build a prefix; a single repeated letter cycles through the
 * options starting with it. Returns the matching index (or `active` when nothing matches).
 */
export function typeahead(options: NavOption[], active: number, key: string, state: Typeahead, now: number): number {
  const k = key.toLocaleLowerCase();
  state.text = now - state.at > 600 ? k : state.text + k;
  state.at = now;
  const cycling = state.text.length > 1 && [...state.text].every((c) => c === k);
  const prefix = cycling ? k : state.text;
  const start = prefix.length === 1 ? active + 1 : Math.max(0, active);
  const n = options.length;
  for (let i = 0; i < n; i++) {
    const idx = (start + i) % n;
    const o = options[idx]!;
    if (!o.disabled && o.label.toLocaleLowerCase().startsWith(prefix)) return idx;
  }
  return active;
}

export type ListAction = { kind: 'move'; to: number } | { kind: 'pick'; index: number } | { kind: 'close' } | { kind: 'tab' } | { kind: 'none' };

/** What a key pressed inside the open listbox does. */
export function listKey(options: NavOption[], active: number, key: string, ta: Typeahead, now: number, mods: { alt?: boolean; ctrl?: boolean; meta?: boolean } = {}): ListAction {
  switch (key) {
    case 'ArrowDown': return { kind: 'move', to: active < 0 ? Math.max(0, firstEnabled(options, 0, 1)) : step(options, active, 1) };
    case 'ArrowUp':
      if (mods.alt) return active >= 0 ? { kind: 'pick', index: active } : { kind: 'close' };
      return { kind: 'move', to: active < 0 ? Math.max(0, firstEnabled(options, options.length - 1, -1)) : step(options, active, -1) };
    case 'Home': return { kind: 'move', to: Math.max(0, firstEnabled(options, 0, 1)) };
    case 'End': return { kind: 'move', to: Math.max(0, firstEnabled(options, options.length - 1, -1)) };
    case 'PageDown': { let i = active; for (let n = 0; n < 8; n++) i = step(options, i, 1); return { kind: 'move', to: i }; }
    case 'PageUp': { let i = active; for (let n = 0; n < 8; n++) i = step(options, i, -1); return { kind: 'move', to: i }; }
    case 'Enter': case ' ':
      if (key === ' ' && ta.text && now - ta.at <= 600) return { kind: 'move', to: typeahead(options, active, key, ta, now) };
      return active >= 0 && !options[active]?.disabled ? { kind: 'pick', index: active } : { kind: 'none' };
    case 'Escape': return { kind: 'close' };
    case 'Tab': return { kind: 'tab' };
    default:
      if (key.length === 1 && /\S/.test(key) && !mods.ctrl && !mods.meta && !mods.alt) return { kind: 'move', to: typeahead(options, active, key, ta, now) };
      return { kind: 'none' };
  }
}
