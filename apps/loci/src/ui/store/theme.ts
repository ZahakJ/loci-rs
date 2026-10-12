/** Theme preference → resolved theme, persisted per device in localStorage 'loci.theme'. Pure parts are tested. */
export type ThemePref = 'system' | 'light' | 'dark';
export type ResolvedTheme = 'light' | 'dark';
export const THEME_KEY = 'loci.theme';

export function isThemePref(v: unknown): v is ThemePref { return v === 'system' || v === 'light' || v === 'dark'; }
export function resolveTheme(pref: ThemePref, systemDark: boolean): ResolvedTheme {
  return pref === 'system' ? (systemDark ? 'dark' : 'light') : pref;
}
export function readStoredTheme(storage: Pick<Storage, 'getItem'> | null): ThemePref {
  try { const v = storage?.getItem(THEME_KEY); return isThemePref(v) ? v : 'system'; } catch { return 'system'; }
}
export function storeTheme(storage: Pick<Storage, 'setItem' | 'removeItem'> | null, pref: ThemePref): void {
  try { if (pref === 'system') storage?.removeItem(THEME_KEY); else storage?.setItem(THEME_KEY, pref); } catch { /* storage blocked */ }
}
export function systemPrefersDark(): boolean {
  return typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: dark)').matches;
}

let fadeTimer: number | undefined;
/** Applies a resolved theme to <html>; `fade` adds the --d-base (160ms) colour cross-fade. */
export function applyTheme(theme: ResolvedTheme, fade = false): void {
  const html = document.documentElement;
  if (html.dataset.theme === theme) return;
  if (fade) {
    html.classList.add('theme-fade');
    window.clearTimeout(fadeTimer);
    fadeTimer = window.setTimeout(() => html.classList.remove('theme-fade'), 220);
  }
  html.dataset.theme = theme;
  const meta = document.querySelector('meta[name="theme-color"]');
  if (meta) meta.setAttribute('content', getComputedStyle(html).getPropertyValue('--bg').trim() || (theme === 'dark' ? 'oklch(17% 0.015 260)' : 'oklch(97.5% 0.006 85)'));
}
