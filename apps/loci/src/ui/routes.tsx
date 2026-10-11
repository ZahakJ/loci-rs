/**
 * Hash router (DESIGN §4: no router library). `parseRoute` / `formatRoute` are pure and tested.
 */
import { lazy, useSyncExternalStore } from 'react';

export type Route =
  | { name: 'home' }
  | { name: 'welcome' }
  | { name: 'study'; deckId: number | null; custom?: string }
  | { name: 'backlog'; deckId: number | null }
  | { name: 'browse'; q?: string }
  | { name: 'stats'; deckId: number | null }
  | { name: 'settings'; section?: string }
  | { name: 'preset'; presetId: number }
  | { name: 'notetypes' }
  | { name: 'import' }
  | { name: 'backups' }
  | { name: 'note'; noteId: number }
  | { name: 'add'; deckId?: number | null }
  | { name: 'notFound'; path: string };

export type RouteName = Route['name'];

function deckParam(seg: string | undefined): number | null | undefined {
  if (seg === undefined || seg === '' || seg === 'all') return null;
  return /^\d+$/.test(seg) ? Number(seg) : undefined;
}

/** Parses `#/path?query` (or a bare path) into a Route. Unknown paths yield `notFound`. */
export function parseRoute(hash: string): Route {
  let raw = hash.startsWith('#') ? hash.slice(1) : hash;
  if (!raw.startsWith('/')) raw = '/' + raw;
  const qi = raw.indexOf('?');
  const path = qi >= 0 ? raw.slice(0, qi) : raw;
  const params = new URLSearchParams(qi >= 0 ? raw.slice(qi + 1) : '');
  const segs = path.split('/').filter(Boolean).map((p) => { try { return decodeURIComponent(p); } catch { return p; } });
  const [a, b, c] = segs;
  const nf: Route = { name: 'notFound', path };
  switch (a) {
    case undefined: return { name: 'home' };
    case 'welcome': return segs.length === 1 ? { name: 'welcome' } : nf;
    case 'study':
    case 'backlog':
    case 'stats': {
      if (segs.length > 2) return nf;
      const deckId = deckParam(b);
      if (deckId === undefined) return nf;
      if (a === 'study') {
        const custom = params.get('custom');
        return custom ? { name: 'study', deckId, custom } : { name: 'study', deckId };
      }
      return { name: a, deckId };
    }
    case 'browse': {
      if (segs.length > 1) return nf;
      const q = params.get('q');
      return q !== null ? { name: 'browse', q } : { name: 'browse' };
    }
    case 'settings':
      if (b === undefined) {
        const section = params.get('section');
        return section ? { name: 'settings', section } : { name: 'settings' };
      }
      if (b === 'presets' && c !== undefined && /^\d+$/.test(c) && segs.length === 3) return { name: 'preset', presetId: Number(c) };
      if (b === 'notetypes' && segs.length === 2) return { name: 'notetypes' };
      return nf;
    case 'import': return segs.length === 1 ? { name: 'import' } : nf;
    case 'backups': return segs.length === 1 ? { name: 'backups' } : nf;
    case 'note': return segs.length === 2 && b !== undefined && /^\d+$/.test(b) ? { name: 'note', noteId: Number(b) } : nf;
    case 'add': {
      if (segs.length > 1) return nf;
      const d = params.get('deck');
      return d && /^\d+$/.test(d) ? { name: 'add', deckId: Number(d) } : { name: 'add' };
    }
    default: return nf;
  }
}

/** Inverse of parseRoute: returns a `#/...` hash. */
export function formatRoute(r: Route): string {
  const deck = (id: number | null) => (id === null ? 'all' : String(id));
  switch (r.name) {
    case 'home': return '#/';
    case 'welcome': return '#/welcome';
    case 'study': return `#/study/${deck(r.deckId)}${r.custom ? `?custom=${encodeURIComponent(r.custom)}` : ''}`;
    case 'backlog': return `#/backlog/${deck(r.deckId)}`;
    case 'stats': return `#/stats/${deck(r.deckId)}`;
    case 'browse': return r.q !== undefined && r.q !== '' ? `#/browse?q=${encodeURIComponent(r.q)}` : '#/browse';
    case 'settings': return r.section ? `#/settings?section=${encodeURIComponent(r.section)}` : '#/settings';
    case 'preset': return `#/settings/presets/${r.presetId}`;
    case 'notetypes': return '#/settings/notetypes';
    case 'import': return '#/import';
    case 'backups': return '#/backups';
    case 'note': return `#/note/${r.noteId}`;
    case 'add': return r.deckId != null ? `#/add?deck=${r.deckId}` : '#/add';
    case 'notFound': return `#${r.path}`;
  }
}

/** Which primary nav item a route belongs to (for highlighting). */
export function navSection(r: Route): 'home' | 'browse' | 'stats' | 'settings' | null {
  switch (r.name) {
    case 'home': case 'backlog': return 'home';
    case 'browse': case 'note': return 'browse';
    case 'stats': return 'stats';
    case 'settings': case 'preset': case 'notetypes': case 'import': case 'backups': return 'settings';
    default: return null;
  }
}

/** Routes rendered without any navigation chrome. */
export function isFullBleed(r: Route): boolean {
  return r.name === 'study' || r.name === 'welcome';
}

// ── browser binding ──
const subscribe = (cb: () => void) => { window.addEventListener('hashchange', cb); return () => window.removeEventListener('hashchange', cb); };
const snapshot = () => window.location.hash || '#/';

export function useHash(): string { return useSyncExternalStore(subscribe, snapshot, () => '#/'); }
export function useRoute(): Route { return parseRoute(useHash()); }

/** Navigates to a hash (string or Route). `replace` avoids a history entry. */
export function navigate(to: string | Route, opts: { replace?: boolean } = {}): void {
  const hash = typeof to === 'string' ? (to.startsWith('#') ? to : `#${to}`) : formatRoute(to);
  if (window.location.hash === hash) return;
  if (opts.replace) {
    replacing = true;
    history.replaceState(history.state, '', `${location.pathname}${location.search}${hash}`);
    window.dispatchEvent(new HashChangeEvent('hashchange'));
  } else {
    window.location.hash = hash;
  }
}

/** Goes back when the previous entry is ours, otherwise to `fallback`. */
// In-app history, so "back" never leaves the app.
const trail: string[] = typeof window !== 'undefined' ? [snapshot()] : [];
let replacing = false;
if (typeof window !== 'undefined') {
  window.addEventListener('hashchange', () => {
    const h = snapshot();
    if (replacing) { trail[trail.length - 1] = h; replacing = false; return; }
    if (trail.length > 1 && trail[trail.length - 2] === h) trail.pop();
    else trail.push(h);
  });
}
/** The previous in-app hash, if any. */
export function previousHash(): string | null { return trail.length > 1 ? trail[trail.length - 2]! : null; }
export function goBack(fallback = '#/'): void {
  if (trail.length > 1) history.back();
  else navigate(fallback, { replace: true });
}

// ── lazy views owned by other agents (exact paths; they replace the placeholders) ──
export const StudyView = lazy(() => import('./views/study/StudyView'));
export const BacklogView = lazy(() => import('./views/backlog/BacklogView'));
export const BrowseView = lazy(() => import('./views/browse/BrowseView'));
export const StatsView = lazy(() => import('./views/stats/StatsView'));
export const EditNoteView = lazy(() => import('./views/edit/EditNoteView'));
export const NotetypesView = lazy(() => import('./views/notetypes/NotetypesView'));
export const AddSheet = lazy(() => import('./views/edit/AddSheet').then((m) => ({ default: m.AddSheet })));
// ── views owned by the shell agent ──
export const HomeView = lazy(() => import('./views/home/HomeView'));
export const WelcomeView = lazy(() => import('./views/welcome/WelcomeView'));
export const SettingsView = lazy(() => import('./views/settings/SettingsView'));
export const PresetEditorView = lazy(() => import('./views/settings/PresetEditorView'));
export const ImportView = lazy(() => import('./views/import/ImportView'));
export const BackupsView = lazy(() => import('./views/backups/BackupsView'));
