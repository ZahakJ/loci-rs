/**
 * Sync from the UI's side: the folder this device uses (native path or browser folder), *Sync
 * now*, automatic syncs (on open, when the window closes or the page is hidden, after a study
 * session) and the sync-file hand-off. docs/SYNC.md has the design.
 */
import { useSyncExternalStore } from 'react';
import { t } from '@core/i18n/i18n';
import type { SyncReport, SyncStatus } from '@core/types';
import { api, downloadBytes, errorMessage } from '../api';
import { browserFolderIO, folderPermission, loadBrowserFolder, pickBrowserFolder, storeBrowserFolder, supportsBrowserFolder } from './browserFolder';
import { syncFileName } from './describe';
import { syncFolder, type FolderSyncResult, type SyncFolderIO } from './folderSync';

export { describeSync, syncFileName } from './describe';

export type FolderKind = 'native' | 'browser' | null;

function inTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
function onAndroid(): boolean {
  return typeof navigator !== 'undefined' && /Android/i.test(navigator.userAgent);
}

/** How this device can reach a sync folder: the desktop app's native folders, a browser folder, or not at all. */
export function folderKind(): FolderKind {
  if (inTauri()) return onAndroid() ? null : 'native';
  return supportsBrowserFolder() ? 'browser' : null;
}

export interface SyncState { syncing: boolean; last: FolderSyncResult | null; error: string | null; at: number }
let state: SyncState = { syncing: false, last: null, error: null, at: 0 };
const listeners = new Set<() => void>();
function setState(patch: Partial<SyncState>): void {
  state = { ...state, ...patch };
  for (const l of listeners) l();
}
export function useSyncState(): SyncState {
  return useSyncExternalStore((cb) => { listeners.add(cb); return () => { listeners.delete(cb); }; }, () => state, () => state);
}

async function folderIO(status: SyncStatus, ask: boolean): Promise<SyncFolderIO | null> {
  const kind = folderKind();
  if (kind === 'native' && status.folder) {
    const { nativeFolderIO } = await import('./nativeFolder');
    return nativeFolderIO(status.folder);
  }
  if (kind === 'browser' && status.folder) {
    const h = await loadBrowserFolder();
    if (!h || !(await folderPermission(h, ask))) return null;
    return browserFolderIO(h);
  }
  return null;
}

/** Lets the user pick a sync folder and syncs with it right away. Null when cancelled. */
export async function chooseFolder(): Promise<FolderSyncResult | null> {
  const kind = folderKind();
  let label: string | null = null;
  if (kind === 'native') {
    const { pickNativeFolder } = await import('./nativeFolder');
    label = await pickNativeFolder();
  } else if (kind === 'browser') {
    const h = await pickBrowserFolder();
    if (h && (await folderPermission(h, true))) {
      await storeBrowserFolder(h);
      label = h.name;
    }
  }
  if (!label) return null;
  await api.setSyncFolder(label);
  return syncNow('manual');
}

export async function forgetFolder(): Promise<SyncStatus> {
  if (folderKind() === 'browser') await storeBrowserFolder(null).catch(() => undefined);
  return api.setSyncFolder(null);
}

let running: Promise<FolderSyncResult | null> | null = null;

/**
 * Syncs with the folder, if this device has one. One sync at a time; a call while one runs
 * waits for it. Automatic syncs never ask for permission and keep quiet on failure.
 */
export function syncNow(reason: 'manual' | 'open' | 'close' | 'study'): Promise<FolderSyncResult | null> {
  if (running) return running;
  running = (async () => {
    const status = await api.syncStatus();
    if (!status.folder) return null;
    const io = await folderIO(status, reason === 'manual');
    if (!io) {
      if (reason === 'manual') throw new Error(t('sync.noAccess'));
      return null;
    }
    setState({ syncing: true, error: null });
    try {
      const last = await syncFolder(api, io);
      setState({ syncing: false, last, at: Date.now() });
      return last;
    } catch (e) {
      setState({ syncing: false, error: errorMessage(e) });
      if (reason === 'manual') throw e;
      return null;
    }
  })().finally(() => { running = null; });
  return running;
}

/** Saves this device's sync file (share sheet / downloads on phones, a save dialog on desktop). */
export async function saveSyncFile(): Promise<void> {
  const bytes = await api.exportSyncBundle();
  downloadBytes(bytes, syncFileName(), 'application/octet-stream');
}

/** Merges a sync file from another device. */
export async function openSyncFile(file: Blob): Promise<SyncReport> {
  return api.mergeSyncBundle(new Uint8Array(await file.arrayBuffer()));
}

let installed = false;

/** Desktop app and browser: sync on open, when leaving, and after study sessions (when a folder is set). */
export function installAutoSync(): void {
  if (installed || !folderKind()) return;
  installed = true;
  void syncNow('open');
  if (folderKind() === 'native') {
    void import('@tauri-apps/api/window').then(({ getCurrentWindow }) =>
      getCurrentWindow().onCloseRequested(async () => {
        await Promise.race([syncNow('close').catch(() => null), new Promise((r) => setTimeout(r, 20_000))]);
      })).catch(() => undefined);
  } else {
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'hidden') void syncNow('close').catch(() => null);
    });
  }
}

/** Call when a study session ends. */
export function syncAfterStudy(): void {
  if (folderKind()) void syncNow('study').catch(() => null);
}
