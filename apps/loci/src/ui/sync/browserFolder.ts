/**
 * A sync folder in the browser (File System Access API: Chrome, Edge and other Chromium
 * browsers on desktop). The folder handle is kept in IndexedDB; the browser asks again for
 * permission after a restart, so automatic syncs only run while permission is granted.
 */
import type { FolderEntry, SyncFolderIO } from './folderSync';

type Mode = { mode: 'readwrite' };
interface DirHandle {
  readonly kind: 'directory';
  readonly name: string;
  getDirectoryHandle(name: string, opts?: { create?: boolean }): Promise<DirHandle>;
  getFileHandle(name: string, opts?: { create?: boolean }): Promise<FileHandle>;
  removeEntry(name: string): Promise<void>;
  entries(): AsyncIterable<[string, { kind: 'file' | 'directory' }]>;
  queryPermission(opts: Mode): Promise<PermissionState>;
  requestPermission(opts: Mode): Promise<PermissionState>;
}
interface FileHandle {
  getFile(): Promise<File>;
  createWritable(): Promise<{ write(data: BufferSource): Promise<void>; close(): Promise<void> }>;
}
export type BrowserFolder = DirHandle;

export function supportsBrowserFolder(): boolean {
  return typeof window !== 'undefined' && 'showDirectoryPicker' in window && typeof indexedDB !== 'undefined';
}

export function browserFolderIO(root: BrowserFolder): SyncFolderIO {
  const dir = async (path: string[], create: boolean): Promise<DirHandle | null> => {
    let d = root;
    for (const p of path) {
      try { d = await d.getDirectoryHandle(p, { create }); } catch { return null; }
    }
    return d;
  };
  return {
    async list(path) {
      const d = await dir(path, false);
      if (!d) return [];
      const out: FolderEntry[] = [];
      for await (const [name, h] of d.entries()) out.push({ name, dir: h.kind === 'directory' });
      return out;
    },
    async read(path) {
      const d = await dir(path.slice(0, -1), false);
      if (!d) return null;
      try {
        const f = await (await d.getFileHandle(path[path.length - 1]!)).getFile();
        return new Uint8Array(await f.arrayBuffer());
      } catch { return null; }
    },
    async write(path, bytes) {
      const d = await dir(path.slice(0, -1), true);
      if (!d) throw new Error('The sync folder cannot be written');
      const w = await (await d.getFileHandle(path[path.length - 1]!, { create: true })).createWritable();
      await w.write(bytes as BufferSource);
      await w.close();
    },
    async remove(path) {
      const d = await dir(path.slice(0, -1), false);
      await d?.removeEntry(path[path.length - 1]!).catch(() => undefined);
    },
  };
}

export async function pickBrowserFolder(): Promise<BrowserFolder | null> {
  const w = window as unknown as { showDirectoryPicker(opts: { id: string; mode: 'readwrite' }): Promise<DirHandle> };
  try { return await w.showDirectoryPicker({ id: 'loci-sync', mode: 'readwrite' }); } catch { return null; }
}

/** True when the folder may be read and written; asks the user only when `ask` (needs a click). */
export async function folderPermission(h: BrowserFolder, ask: boolean): Promise<boolean> {
  const opts: Mode = { mode: 'readwrite' };
  try {
    if ((await h.queryPermission(opts)) === 'granted') return true;
    return ask && (await h.requestPermission(opts)) === 'granted';
  } catch { return false; }
}

const DB = 'loci-sync';
const STORE = 'folder';
const KEY = 'handle';

function open(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function tx<T>(mode: IDBTransactionMode, run: (s: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  const db = await open();
  try {
    return await new Promise<T>((resolve, reject) => {
      const req = run(db.transaction(STORE, mode).objectStore(STORE));
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error);
    });
  } finally { db.close(); }
}

export async function loadBrowserFolder(): Promise<BrowserFolder | null> {
  if (!supportsBrowserFolder()) return null;
  try { return ((await tx('readonly', (s) => s.get(KEY))) as BrowserFolder | undefined) ?? null; } catch { return null; }
}
export async function storeBrowserFolder(h: BrowserFolder | null): Promise<void> {
  if (h) await tx('readwrite', (s) => s.put(h, KEY));
  else await tx('readwrite', (s) => s.delete(KEY));
}
