/**
 * Tauri transport: `invoke` into the Rust commands defined in `src-tauri/src/lib.rs`.
 *
 *   stage_bytes(raw body) -> token              bytes for previewImport / importPackage / addMedia
 *   rpc(method, args, token?, onProgress)       JSON in, JSON out
 *   rpc_bytes(method, args, onProgress)         JSON in, raw bytes out (empty = null)
 *   save_bytes(raw body; headers: filename)     save dialog (desktop) or app downloads dir (Android)
 */
import { Channel, invoke } from '@tauri-apps/api/core';
import type { Progress } from '@core/types';
import type { Transport } from './index';
import type { RpcMethod } from './protocol';

export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

function progressChannel(onProgress?: (p: Progress) => void): Channel<Progress> {
  const ch = new Channel<Progress>();
  ch.onmessage = (p) => onProgress?.(p);
  return ch;
}

async function stage(bytes: Uint8Array): Promise<string> {
  // A fresh ArrayBuffer-backed copy: Tauri sends the raw body without JSON encoding.
  const body = bytes.byteOffset === 0 && bytes.byteLength === bytes.buffer.byteLength ? bytes : bytes.slice();
  return invoke<string>('stage_bytes', body);
}

export function createTauriTransport(): Transport {
  return {
    kind: 'tauri',
    async call(method: RpcMethod, args: unknown[], bytes: Uint8Array | null, onProgress?: (p: Progress) => void): Promise<unknown> {
      const token = bytes ? await stage(bytes) : null;
      return invoke<unknown>('rpc', { method, args, token, onProgress: progressChannel(onProgress) });
    },
    async callBytes(method: RpcMethod, args: unknown[], onProgress?: (p: Progress) => void): Promise<Uint8Array | null> {
      const out = await invoke<ArrayBuffer | number[]>('rpc_bytes', { method, args, onProgress: progressChannel(onProgress) });
      const arr = out instanceof ArrayBuffer ? new Uint8Array(out) : new Uint8Array(out);
      return arr.byteLength ? arr : null;
    },
    async saveFile(bytes: Uint8Array, filename: string, _mime: string): Promise<string | null> {
      const body = bytes.byteOffset === 0 && bytes.byteLength === bytes.buffer.byteLength ? bytes : bytes.slice();
      return invoke<string | null>('save_bytes', body, { headers: { 'x-loci-filename': encodeURIComponent(filename) } });
    },
  };
}
