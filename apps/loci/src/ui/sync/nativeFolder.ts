/**
 * A native sync folder through the Tauri shell (`sync_folder_*` commands in src-tauri/src/lib.rs).
 * The shell only touches paths inside `<folder>/loci-sync/`.
 */
import { invoke } from '@tauri-apps/api/core';
import type { FolderEntry, SyncFolderIO } from './folderSync';

function body(bytes: Uint8Array): Uint8Array {
  // A fresh ArrayBuffer-backed copy: Tauri sends the raw body without JSON encoding.
  return bytes.byteOffset === 0 && bytes.byteLength === bytes.buffer.byteLength ? bytes : bytes.slice();
}

export function nativeFolderIO(root: string): SyncFolderIO {
  return {
    list: (path) => invoke<FolderEntry[]>('sync_folder_list', { root, path }),
    async read(path) {
      const out = await invoke<ArrayBuffer | number[] | null>('sync_folder_read', { root, path });
      if (!out) return null;
      const arr = out instanceof ArrayBuffer ? new Uint8Array(out) : new Uint8Array(out);
      return arr.byteLength ? arr : null;
    },
    async write(path, bytes) {
      await invoke('sync_folder_write', body(bytes), {
        headers: { 'x-loci-root': encodeURIComponent(root), 'x-loci-path': encodeURIComponent(JSON.stringify(path)) },
      });
    },
    async remove(path) { await invoke('sync_folder_remove', { root, path }); },
  };
}

/** Native folder picker; null when cancelled. */
export function pickNativeFolder(): Promise<string | null> {
  return invoke<string | null>('pick_sync_folder');
}
