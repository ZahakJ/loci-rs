/**
 * Folder sync (docs/SYNC.md "Transports"): reads every other device's file in the sync folder,
 * merges it, takes the media this device lacks, writes this device's own file and the media the
 * folder lacks. The same steps run over a native folder (Tauri commands) and a browser folder
 * (File System Access); both implement `SyncFolderIO`.
 *
 *   <folder>/loci-sync/<collection-id>/<device-id>.loci-sync
 *   <folder>/loci-sync/<collection-id>/media/<sha1>
 */
import type { CollectionApi } from '@core/api';
import type { SyncReport } from '@core/types';

export interface FolderEntry { name: string; dir: boolean }

/** Folder access relative to the chosen folder; paths are lists of names. */
export interface SyncFolderIO {
  /** Entries of a directory; empty when it does not exist. */
  list(path: string[]): Promise<FolderEntry[]>;
  /** File contents, or null when missing. */
  read(path: string[]): Promise<Uint8Array | null>;
  /** Writes a file whole (replacing it), creating directories as needed. */
  write(path: string[], bytes: Uint8Array): Promise<void>;
  /** Removes a file if present. */
  remove(path: string[]): Promise<void>;
}

export type SyncApi = Pick<CollectionApi, 'syncStatus' | 'exportSyncBundle' | 'mergeSyncBundle' | 'syncMediaState' | 'addSyncMedia' | 'getMedia'>;

export const SYNC_DIR = 'loci-sync';
export const SYNC_EXT = '.loci-sync';
const DEVICE_FILE = /^([0-9a-f]{16})\.loci-sync$/;
const COLLECTION_DIR = /^[0-9a-zA-Z-]{1,64}$/;
const SHA1 = /^[0-9a-f]{40}$/;

export interface FolderSyncResult {
  /** Other devices whose files were merged. */
  devices: number;
  rowsIn: number;
  conflictsResolved: number;
  reviewsAdded: number;
  /** Media files taken from / copied to the folder. */
  mediaIn: number;
  mediaOut: number;
  /** Files that could not be read (half-written by the sync tool, damaged); tried again next time. */
  skipped: number;
  /** A device left things out that this one had not seen; the next sync fills them in. */
  incomplete: boolean;
}

/** The device id named by a sync file, or null for anything else (conflict copies, temp files). */
export function deviceOfFile(name: string): string | null {
  return DEVICE_FILE.exec(name)?.[1] ?? null;
}

export async function syncFolder(api: SyncApi, io: SyncFolderIO): Promise<FolderSyncResult> {
  const res: FolderSyncResult = { devices: 0, rowsIn: 0, conflictsResolved: 0, reviewsAdded: 0, mediaIn: 0, mediaOut: 0, skipped: 0, incomplete: false };
  let status = await api.syncStatus();
  const mine = new Set([status.deviceId, ...status.formerDevices].filter((d): d is string => !!d));
  const dirs = (await io.list([SYNC_DIR])).filter((e) => e.dir && COLLECTION_DIR.test(e.name)).map((e) => e.name).sort();

  // 1. Merge every other device's file. Every device with a file counts as a reader of ours,
  // even when its file could not be read this time.
  const peers = new Set<string>();
  const stale: string[][] = [];
  for (const dir of dirs) {
    for (const e of await io.list([SYNC_DIR, dir])) {
      const dev = e.dir ? null : deviceOfFile(e.name);
      if (!dev) continue;
      if (mine.has(dev)) { stale.push([SYNC_DIR, dir, e.name]); continue; }
      peers.add(dev);
      let report: SyncReport;
      try {
        const bytes = await io.read([SYNC_DIR, dir, e.name]);
        if (!bytes) continue;
        report = await api.mergeSyncBundle(bytes);
      } catch {
        res.skipped++;
        continue;
      }
      res.devices++;
      res.rowsIn += report.rowsIn;
      res.conflictsResolved += report.conflictsResolved;
      res.reviewsAdded += report.reviewsAdded;
      res.mediaIn += report.mediaAdded;
      if (!report.complete) res.incomplete = true;
    }
  }

  // Merging may have adopted another collection id; this device writes where the others are.
  status = await api.syncStatus();
  const target = status.collectionId;
  const own = status.deviceId;

  // 2. Media this device lacks.
  const media = await api.syncMediaState();
  const wanted = [...new Set(media.need.map((m) => m.sha1))].filter((s) => SHA1.test(s));
  const searchDirs = [target, ...dirs.filter((d) => d !== target)];
  for (const sha1 of wanted) {
    for (const dir of searchDirs) {
      const bytes = await io.read([SYNC_DIR, dir, 'media', sha1]).catch(() => null);
      if (!bytes) continue;
      try { res.mediaIn += await api.addSyncMedia(sha1, bytes); break; } catch { /* damaged copy: try the next */ }
    }
  }

  // 3. This device's file.
  const bundle = await api.exportSyncBundle({ folder: true, peers: [...peers] });
  if (!own) return res;
  await io.write([SYNC_DIR, target, `${own}${SYNC_EXT}`], bundle);
  for (const path of stale) {
    if (path[1] === target && path[2] === `${own}${SYNC_EXT}`) continue;
    await io.remove(path).catch(() => undefined);
  }

  // 4. Media the folder lacks.
  const present = new Set((await io.list([SYNC_DIR, target, 'media'])).filter((e) => !e.dir).map((e) => e.name));
  const sent = new Set<string>();
  for (const m of media.have) {
    if (!SHA1.test(m.sha1) || present.has(m.sha1) || sent.has(m.sha1)) continue;
    const bytes = await api.getMedia(m.name);
    if (!bytes) continue;
    await io.write([SYNC_DIR, target, 'media', m.sha1], bytes);
    sent.add(m.sha1);
    res.mediaOut++;
  }
  return res;
}

/** In-memory folder, for tests and previews. */
export function memoryFolder(files: Map<string, Uint8Array> = new Map()): SyncFolderIO & { files: Map<string, Uint8Array> } {
  const key = (p: string[]) => p.join('/');
  return {
    files,
    async list(path) {
      const prefix = path.length ? `${key(path)}/` : '';
      const out = new Map<string, boolean>();
      for (const k of files.keys()) {
        if (!k.startsWith(prefix)) continue;
        const rest = k.slice(prefix.length).split('/');
        out.set(rest[0]!, rest.length > 1 || out.get(rest[0]!) === true);
      }
      return [...out].map(([name, dir]) => ({ name, dir }));
    },
    async read(path) { return files.get(key(path)) ?? null; },
    async write(path, bytes) { files.set(key(path), bytes); },
    async remove(path) { files.delete(key(path)); },
  };
}
