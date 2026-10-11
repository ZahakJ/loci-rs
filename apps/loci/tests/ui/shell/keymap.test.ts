import { describe, expect, it } from 'vitest';
import { SHORTCUT_GROUPS, matchGlobal, type KeyInput } from '@ui/keys/keymap';
import en from '@core/i18n/en.json';

const key = (k: string, mods: Partial<KeyInput> = {}): KeyInput => ({ key: k, ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, editable: false, ...mods });
const home = { route: 'home' as const, modalOpen: false };

describe('matchGlobal', () => {
  it('maps the single-letter shortcuts', () => {
    expect(matchGlobal(key('a'), home)).toBe('add');
    expect(matchGlobal(key('b'), home)).toBe('browse');
    expect(matchGlobal(key('s'), home)).toBe('stats');
    expect(matchGlobal(key('d'), home)).toBe('home');
    expect(matchGlobal(key('/'), home)).toBe('search');
    expect(matchGlobal(key('?', { shiftKey: true }), home)).toBe('help');
  });
  it('maps ctrl/cmd+z to undo', () => {
    expect(matchGlobal(key('z', { ctrlKey: true }), home)).toBe('undo');
    expect(matchGlobal(key('Z', { metaKey: true }), home)).toBe('undo');
    expect(matchGlobal(key('z', { ctrlKey: true, shiftKey: true }), home)).toBeNull();
  });
  it('ignores typing, modals, handled and repeated events', () => {
    expect(matchGlobal(key('a', { editable: true }), home)).toBeNull();
    expect(matchGlobal(key('z', { ctrlKey: true, editable: true }), home)).toBeNull();
    expect(matchGlobal(key('a'), { ...home, modalOpen: true })).toBeNull();
    expect(matchGlobal(key('a', { defaultPrevented: true }), home)).toBeNull();
    expect(matchGlobal(key('a', { repeat: true }), home)).toBeNull();
  });
  it('leaves Study keys to the Study view except help', () => {
    const study = { route: 'study' as const, modalOpen: false };
    expect(matchGlobal(key('s'), study)).toBeNull();
    expect(matchGlobal(key('b'), study)).toBeNull();
    expect(matchGlobal(key('z', { ctrlKey: true }), study)).toBeNull();
    expect(matchGlobal(key('?', { shiftKey: true }), study)).toBe('help');
  });
  it('ignores modified letters and shift+letters', () => {
    expect(matchGlobal(key('a', { ctrlKey: true }), home)).toBeNull();
    expect(matchGlobal(key('A', { shiftKey: true }), home)).toBeNull();
    expect(matchGlobal(key('a', { altKey: true }), home)).toBeNull();
  });
});

describe('shortcut help', () => {
  it('has an English label for every listed shortcut', () => {
    const dict = flat(en);
    for (const g of SHORTCUT_GROUPS) {
      expect(dict[g.title], g.title).toBeTruthy();
      for (const it of g.items) expect(dict[it.label], it.label).toBeTruthy();
    }
  });
  it('lists every study key from DESIGN §8', () => {
    const study = SHORTCUT_GROUPS.find((g) => g.id === 'study')!.items.flatMap((i) => i.keys);
    for (const k of ['space', 'enter', '1', '2', '3', '4', 'z', 'e', 's', 'b', 'f', 'm', 'i', 'r', 't', 'esc']) expect(study).toContain(k);
    // every alias is listed too
    for (const k of ['u', 'ctrl+z', 'o', '@', '!', '-', '=', '*', 'ctrl+delete', 'ctrl+shift+d']) expect(study).toContain(k);
  });
});

function flat(obj: unknown, prefix = '', out: Record<string, string> = {}): Record<string, string> {
  for (const [k, v] of Object.entries(obj as Record<string, unknown>)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (typeof v === 'string') out[key] = v; else if (v && typeof v === 'object') flat(v, key, out);
  }
  return out;
}
