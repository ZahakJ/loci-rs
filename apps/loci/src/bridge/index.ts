/**
 * The UI's door to the Rust engine. `createBridgeApi()` returns a `CollectionApi` whose every
 * method is one RPC call over the active transport:
 *
 * - **Tauri** (the desktop and Android apps): `invoke('rpc', …)`, byte payloads staged through
 *   `invoke('stage_bytes', bytes)`, byte results through `invoke('rpc_bytes', …)`, progress through
 *   a `Channel`, files saved through `invoke('save_bytes', …)` (native dialog on desktop).
 * - **HTTP** (`loci serve`, used by `pnpm dev` without Tauri and by the Playwright suite): the same
 *   calls as JSON over `fetch` to a local port.
 *
 * Events: after a successful call the bridge emits `changed` for whatever the protocol table says the
 * method may have touched, `backup` after backups, `optimised` after maintenance, and `error` when a
 * call fails (callers also get the rejection). Media object URLs are cached per name here, like the
 * worker client did, and dropped when the collection is replaced.
 */
import { LociError, type CollectionApi, type CollectionEvent, type LociErrorCode } from '@core/api';
import type { BackupInfo, OptimiseResult, Progress } from '@core/types';
import { BYTES_IN, BYTES_IN_SECOND, BYTES_OUT, CHANGES, isRpcMethod, type RpcMethod, type SerializedError } from './protocol';
import { createTauriTransport, isTauri } from './tauri';
import { createHttpTransport, httpBridgeUrl } from './http';

export type EventType = CollectionEvent['type'] | '*';
export interface BridgeEvents { on(type: EventType, cb: (e: CollectionEvent) => void): () => void }
export type BridgeCollectionApi = CollectionApi & { getMediaUrl(name: string): Promise<string | null> };
export interface BridgeHandle { api: BridgeCollectionApi; events: BridgeEvents; transport: Transport }

/** What a transport must do. Bytes travel as `Uint8Array`; JSON as plain values. */
export interface Transport {
  readonly kind: 'tauri' | 'http';
  /** JSON call. `bytes` is the staged payload for BYTES_IN methods. */
  call(method: RpcMethod, args: unknown[], bytes: Uint8Array | null, onProgress?: (p: Progress) => void): Promise<unknown>;
  /** Call returning bytes (null when the engine returned nothing). */
  callBytes(method: RpcMethod, args: unknown[], onProgress?: (p: Progress) => void): Promise<Uint8Array | null>;
  /** Saves bytes as a file the user can find; resolves with a description of where (or null if cancelled). */
  saveFile(bytes: Uint8Array, filename: string, mime: string): Promise<string | null>;
}

const REPLACES_COLLECTION = new Set<RpcMethod>(['importPackage', 'restoreBackup', 'resetCollection', 'checkMedia', 'mergeSyncBundle', 'addSyncMedia']);

const MIME: Record<string, string> = {
  png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', gif: 'image/gif', webp: 'image/webp', avif: 'image/avif',
  svg: 'image/svg+xml', bmp: 'image/bmp', ico: 'image/x-icon', tif: 'image/tiff', tiff: 'image/tiff',
  mp3: 'audio/mpeg', m4a: 'audio/mp4', aac: 'audio/aac', ogg: 'audio/ogg', oga: 'audio/ogg', opus: 'audio/ogg',
  wav: 'audio/wav', flac: 'audio/flac', spx: 'audio/ogg',
  mp4: 'video/mp4', m4v: 'video/mp4', webm: 'video/webm', mov: 'video/quicktime', ogv: 'video/ogg', mkv: 'video/x-matroska',
  ttf: 'font/ttf', otf: 'font/otf', woff: 'font/woff', woff2: 'font/woff2', css: 'text/css',
};

/** MIME type for a media filename (by extension). */
export function mediaMimeType(name: string): string {
  const ext = /\.([a-z0-9]+)$/i.exec(name)?.[1]?.toLowerCase() ?? '';
  return MIME[ext] ?? 'application/octet-stream';
}

export function toLociError(e: unknown): LociError {
  if (e instanceof LociError) return e;
  if (e && typeof e === 'object' && 'code' in e && 'message' in e) {
    const s = e as SerializedError;
    return new LociError(s.code as LociErrorCode, String(s.message), s.detail);
  }
  if (e instanceof Error) return new LociError('internal', e.message, e);
  return new LociError('internal', String(e));
}

class Bridge {
  private readonly listeners = new Map<EventType, Set<(e: CollectionEvent) => void>>();
  readonly mediaUrls = new Map<string, Promise<string | null>>();

  constructor(readonly transport: Transport) {}

  on(type: EventType, cb: (e: CollectionEvent) => void): () => void {
    let set = this.listeners.get(type);
    if (!set) this.listeners.set(type, (set = new Set()));
    set.add(cb);
    return () => { set!.delete(cb); };
  }

  emit(e: CollectionEvent): void {
    for (const key of [e.type, '*'] as EventType[]) for (const cb of this.listeners.get(key) ?? []) {
      try { cb(e); } catch (err) { console.error(err); }
    }
  }

  async call(method: RpcMethod, args: unknown[]): Promise<unknown> {
    let onProgress: ((p: Progress) => void) | undefined;
    const plain = args.filter((a) => {
      if (typeof a === 'function') { onProgress = a as (p: Progress) => void; return false; }
      return true;
    });
    const progress = onProgress ? (p: Progress) => { onProgress!(p); this.emit({ type: 'progress', op: method, progress: p }); } : undefined;
    if (method === 'deleteMedia' && Array.isArray(plain[0])) for (const n of plain[0] as string[]) this.forgetMedia(n);
    try {
      let result: unknown;
      if (BYTES_OUT.has(method)) {
        result = await this.transport.callBytes(method, plain, progress);
      } else if (BYTES_IN.has(method)) {
        const [bytes, ...rest] = plain;
        result = await this.transport.call(method, rest, asBytes(bytes), progress);
      } else if (BYTES_IN_SECOND.has(method)) {
        const [first, bytes, ...rest] = plain;
        result = await this.transport.call(method, [first, ...rest], asBytes(bytes), progress);
      } else {
        result = await this.transport.call(method, plain, null, progress);
      }
      this.afterSuccess(method, result);
      return result;
    } catch (e) {
      const err = toLociError(e);
      this.emit({ type: 'error', error: { code: err.code, message: err.message } });
      throw err;
    }
  }

  private afterSuccess(method: RpcMethod, result: unknown): void {
    if (REPLACES_COLLECTION.has(method)) this.clearMedia();
    const what = CHANGES[method];
    if (what) this.emit({ type: 'changed', what: [...what] });
    if ((method === 'backupNow' || method === 'backupOnLeave' || method === 'autoBackup') && result && typeof result === 'object') {
      this.emit({ type: 'backup', info: result as BackupInfo });
    }
    if (method === 'maintenance' && result && typeof result === 'object') {
      const r = result as { optimised?: OptimiseResult[]; backedUp?: boolean };
      for (const o of r.optimised ?? []) this.emit({ type: 'optimised', result: o });
      if (r.backedUp) this.emit({ type: 'changed', what: ['backups'] });
    }
    if (method === 'optimise' && result && typeof result === 'object') this.emit({ type: 'optimised', result: result as OptimiseResult });
  }

  getMediaUrl(name: string): Promise<string | null> {
    let p = this.mediaUrls.get(name);
    if (!p) {
      p = (this.call('getMedia', [name]) as Promise<Uint8Array | null>).then((bytes) =>
        bytes && bytes.byteLength > 0 ? URL.createObjectURL(new Blob([bytes as BlobPart], { type: mediaMimeType(name) })) : null);
      this.mediaUrls.set(name, p);
      p.catch(() => this.mediaUrls.delete(name));
    }
    return p;
  }

  private forgetMedia(name: string): void {
    const p = this.mediaUrls.get(name);
    if (!p) return;
    this.mediaUrls.delete(name);
    void p.then((u) => { if (u) URL.revokeObjectURL(u); }, () => undefined);
  }

  clearMedia(): void {
    for (const name of [...this.mediaUrls.keys()]) this.forgetMedia(name);
  }
}

function asBytes(v: unknown): Uint8Array {
  if (v instanceof Uint8Array) return v;
  if (v instanceof ArrayBuffer) return new Uint8Array(v);
  throw new LociError('invalid', 'Expected a byte array');
}

let bridge: Bridge | null = null;
let handlePromise: Promise<BridgeHandle> | null = null;

/** Picks the transport: Tauri when running inside the app, otherwise the local HTTP bridge. */
export function selectTransport(): Transport {
  if (isTauri()) return createTauriTransport();
  return createHttpTransport(httpBridgeUrl());
}

/** Connects the UI to the engine (once per page) and returns the API and its event stream. */
export function createBridgeApi(transport: Transport = selectTransport()): Promise<BridgeHandle> {
  handlePromise ??= (async () => {
    const b = new Bridge(transport);
    bridge = b;
    const local: Record<string, unknown> = { getMediaUrl: (name: string) => b.getMediaUrl(name) };
    const api = new Proxy({} as BridgeCollectionApi, {
      get(_t, prop) {
        if (typeof prop === 'symbol') return undefined;
        if (Object.prototype.hasOwnProperty.call(local, prop)) return local[prop];
        if (!isRpcMethod(prop)) return undefined;
        return (...args: unknown[]) => b.call(prop, args);
      },
      has(_t, prop) {
        return typeof prop === 'string' && (Object.prototype.hasOwnProperty.call(local, prop) || isRpcMethod(prop));
      },
    });
    return { api, events: { on: (type, cb) => b.on(type, cb) }, transport };
  })();
  handlePromise.catch(() => { handlePromise = null; });
  return handlePromise;
}

/** The active transport (null before `createBridgeApi`). */
export function activeTransport(): Transport | null { return bridge?.transport ?? null; }

/** Writes an automatic backup now if anything changed since the last one (call when leaving). */
export async function backupOnLeave(): Promise<BackupInfo | null> {
  if (!bridge) return null;
  return (await bridge.call('backupOnLeave', [])) as BackupInfo | null;
}

/**
 * Backs up when the page is hidden or closed (`pagehide`, and `visibilitychange` to hidden, which
 * is the last reliable moment on Android). The Tauri shell also backs up on window close.
 */
export function installLeaveBackup(): () => void {
  let inFlight = false;
  const run = () => {
    if (inFlight || !bridge) return;
    inFlight = true;
    void backupOnLeave().catch(() => null).finally(() => { inFlight = false; });
  };
  const onVisibility = () => { if (document.visibilityState === 'hidden') run(); };
  window.addEventListener('pagehide', run);
  document.addEventListener('visibilitychange', onVisibility);
  return () => {
    window.removeEventListener('pagehide', run);
    document.removeEventListener('visibilitychange', onVisibility);
  };
}

/** Tells the shell the UI is interactive (cold-start measurement); a no-op outside Tauri. */
export function reportReady(): void {
  if (!isTauri()) return;
  void import('@tauri-apps/api/core').then(({ invoke }) => invoke<number>('app_ready')).then((ms) => console.info(`[loci] ready in ${ms} ms`)).catch(() => undefined);
}
