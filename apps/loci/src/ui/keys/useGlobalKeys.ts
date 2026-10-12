import { useEffect, useLayoutEffect, useRef } from 'react';
import { t } from '@core/i18n/i18n';
import { api } from '../api';
import { isModalOpen } from '../design/primitives/layer';
import { toast } from '../design/primitives/Toast';
import { navigate, type Route } from '../routes';
import { useUi } from '../store/ui';
import { isEditable, matchGlobal } from './keymap';

/** Focuses the Browse search box (waits for the lazy view to mount). */
export function focusSearch(tries = 20): void {
  const el = document.querySelector<HTMLInputElement>('[data-global-search], main input[type="search"]');
  if (el) { el.focus(); el.select(); return; }
  if (tries > 0) window.setTimeout(() => focusSearch(tries - 1), 50);
}

export async function runUndo(): Promise<void> {
  try {
    const res = await api.undo();
    toast(res ? { message: t('undo.done', { label: res.label }), icon: 'undo' } : { message: t('undo.nothing'), icon: 'info' });
  } catch (e) {
    toast({ message: t('undo.failed'), tone: 'danger' });
    console.error(e);
  }
}

export function useGlobalKeys(route: Route, onAdd: () => void): void {
  const routeRef = useRef(route);
  const addRef = useRef(onAdd);
  useLayoutEffect(() => { routeRef.current = route; addRef.current = onAdd; });
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const action = matchGlobal(
        { key: e.key, ctrlKey: e.ctrlKey, metaKey: e.metaKey, altKey: e.altKey, shiftKey: e.shiftKey, editable: isEditable(document.activeElement), defaultPrevented: e.defaultPrevented, repeat: e.repeat },
        { route: routeRef.current.name, modalOpen: isModalOpen() },
      );
      if (!action) return;
      e.preventDefault();
      const ui = useUi.getState();
      switch (action) {
        case 'palette': ui.setPaletteOpen(true); break;
        case 'add': addRef.current(); break;
        case 'browse': navigate('#/browse'); break;
        case 'stats': navigate('#/stats/all'); break;
        case 'home': navigate('#/'); break;
        case 'search':
          if (routeRef.current.name !== 'browse') navigate('#/browse');
          focusSearch();
          break;
        case 'help': ui.setHelpOpen(!ui.helpOpen); break;
        case 'undo': void runUndo(); break;
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);
}
