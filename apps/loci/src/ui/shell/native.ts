/**
 * The few places the UI talks to the window rather than the engine: the window's own theme (title
 * bar, native dialogs and menus follow the in-app preference, or the OS while it is "system") and
 * opening links in the system browser. Both are no-ops outside Tauri.
 */
import { isTauri } from '../../bridge/tauri';
import type { ThemePref } from '../store/theme';

export const MANUAL_URL = 'https://zahakj.github.io/loci-rs/manual/';

/** Mirrors the theme preference onto the native window; `system` hands it back to the OS. */
export async function syncWindowTheme(pref: ThemePref): Promise<void> {
  if (!isTauri()) return;
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    await getCurrentWindow().setTheme(pref === 'system' ? null : pref);
  } catch (e) {
    console.warn('set window theme', e);
  }
}

/** Opens a URL in the system browser (Tauri) or a new tab (the web build). */
export async function openExternal(url: string): Promise<void> {
  if (isTauri()) {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('plugin:opener|open_url', { url });
      return;
    } catch (e) {
      console.warn('open url', e);
    }
  }
  window.open(url, '_blank', 'noopener,noreferrer');
}
