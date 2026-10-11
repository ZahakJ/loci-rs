import { describe, expect, it } from 'vitest';
import { THEME_KEY, isThemePref, readStoredTheme, resolveTheme, storeTheme } from '@ui/store/theme';

const mem = () => {
  const m = new Map<string, string>();
  return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v), removeItem: (k: string) => void m.delete(k), m };
};

describe('theme', () => {
  it('resolves system against the OS preference', () => {
    expect(resolveTheme('system', true)).toBe('dark');
    expect(resolveTheme('system', false)).toBe('light');
    expect(resolveTheme('light', true)).toBe('light');
    expect(resolveTheme('dark', false)).toBe('dark');
  });
  it('reads a stored preference, defaulting to system', () => {
    const s = mem();
    expect(readStoredTheme(s)).toBe('system');
    s.setItem(THEME_KEY, 'dark');
    expect(readStoredTheme(s)).toBe('dark');
    s.setItem(THEME_KEY, 'purple');
    expect(readStoredTheme(s)).toBe('system');
    expect(readStoredTheme(null)).toBe('system');
    expect(readStoredTheme({ getItem: () => { throw new Error('blocked'); } })).toBe('system');
  });
  it('stores explicit themes and clears system', () => {
    const s = mem();
    storeTheme(s, 'light');
    expect(s.m.get(THEME_KEY)).toBe('light');
    storeTheme(s, 'system');
    expect(s.m.has(THEME_KEY)).toBe(false);
  });
  it('validates preferences', () => {
    expect(isThemePref('system')).toBe(true);
    expect(isThemePref('auto')).toBe(false);
  });
});
