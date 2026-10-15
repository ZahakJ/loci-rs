/**
 * Browser transport: the Rust engine compiled to WebAssembly runs in a dedicated worker
 * (`wasm.worker.ts`) and keeps the collection in the origin-private file system.
 *
 *   main → worker  {type:'call', id, method, args, bytes, progress}
 *   worker → main  {type:'ready', persistent} | {type:'fatal', error}
 *                  {type:'progress', id, progress} | {type:'result', id, ok, json?, bytes?, error?}
 *
 * One tab owns the collection: a Web Lock is taken before the worker starts; a second tab gets a
 * `busy` error ("Loci is open in another tab") from every call. Bytes travel by structured clone
 * (results are transferred). The first write asks the browser to keep the data
 * (`navigator.storage.persist()`). Files are saved as browser downloads.
 */
import type { Progress } from '@core/types';
import type { Transport } from './index';
import { downloadBlob } from './http';
import { isWrite, type RpcMethod, type SerializedError } from './protocol';

export const LOCK_NAME = 'loci-rs-collection';
export const OTHER_TAB = 'other-tab';

export type ToWorker = { type: 'call'; id: number; method: RpcMethod; args: unknown[]; bytes: Uint8Array | null; progress: boolean };
export type FromWorker =
  | { type: 'ready'; persistent: boolean }
  | { type: 'fatal'; error: SerializedError }
  | { type: 'progress'; id: number; progress: Progress }
  | { type: 'result'; id: number; ok: true; json?: string; bytes?: Uint8Array | null }
  | { type: 'result'; id: number; ok: false; error: SerializedError };

/** The part of `Worker` the transport uses (tests pass a fake). */
export interface WorkerLike {
  postMessage(msg: ToWorker): void;
  terminate(): void;
  addEventListener(type: 'message', cb: (e: MessageEvent<FromWorker>) => void): void;
  addEventListener(type: 'error', cb: (e: ErrorEvent) => void): void;
}

/** The part of `navigator.locks` the transport uses. */
export interface LocksLike {
  request(name: string, options: { ifAvailable: boolean }, cb: (lock: unknown) => Promise<unknown>): Promise<unknown>;
}

export interface WasmTransportOptions {
  spawn: () => WorkerLike;
  locks?: LocksLike | null;
  persist?: (() => Promise<boolean>) | null;
  /** Waits before starting a fresh worker when the storage is still held (tests pass a no-op). */
  delay?: (ms: number) => Promise<void>;
}

/** Engine starts tried while OPFS files are still held, e.g. by the worker of the page being reloaded. */
const STARTS = 6;

export type WasmTransport = Transport & {
  /** Resolves once the engine is running; `persistent` is false when storage is memory only. */
  readonly ready: Promise<{ persistent: boolean }>;
};

const otherTabError = (): SerializedError => ({ code: 'busy', message: 'Loci is open in another tab', detail: OTHER_TAB });

/** Holds the lock for the life of the page; resolves false when another tab has it. */
function takeLock(locks: LocksLike): Promise<boolean> {
  return new Promise<boolean>((resolve, reject) => {
    locks.request(LOCK_NAME, { ifAvailable: true }, (lock) => {
      if (!lock) { resolve(false); return Promise.resolve(); }
      resolve(true);
      return new Promise<never>(() => undefined);
    }).catch(reject);
  });
}

export function createWasmTransport(opts: WasmTransportOptions): WasmTransport {
  const pending = new Map<number, { resolve: (v: { json?: string; bytes?: Uint8Array | null }) => void; reject: (e: SerializedError) => void; onProgress?: (p: Progress) => void }>();
  let nextId = 1;
  let fatal: SerializedError | null = null;
  let persistAsked = false;

  const failAll = (error: SerializedError) => {
    fatal ??= error;
    for (const p of pending.values()) p.reject(error);
    pending.clear();
  };

  const start = () => new Promise<{ worker: WorkerLike; persistent: boolean }>((resolve, reject) => {
    const worker = opts.spawn();
    let started = false;
    const fail = (error: SerializedError) => {
      if (started) failAll(error);
      else { worker.terminate(); reject(error); }
    };
    worker.addEventListener('message', (e: MessageEvent<FromWorker>) => {
      const m = e.data;
      if (m.type === 'ready') { started = true; resolve({ worker, persistent: m.persistent }); return; }
      if (m.type === 'fatal') { fail(m.error); return; }
      const p = pending.get(m.id);
      if (!p) return;
      if (m.type === 'progress') { p.onProgress?.(m.progress); return; }
      pending.delete(m.id);
      if (m.ok) p.resolve(m); else p.reject(m.error);
    });
    worker.addEventListener('error', (e: ErrorEvent) => {
      fail({ code: 'internal', message: `The engine stopped: ${e.message || 'worker error'}` });
    });
  });

  const delay = opts.delay ?? ((ms: number) => new Promise<void>((r) => setTimeout(r, ms)));
  const ready = (async () => {
    if (opts.locks && !(await takeLock(opts.locks))) throw otherTabError();
    for (let attempt = 1; ; attempt++) {
      try {
        return await start();
      } catch (e) {
        const busy = (e as SerializedError | null)?.detail === 'storage-busy';
        if (!busy || attempt >= STARTS) { failAll(e as SerializedError); throw e; }
        await delay(250 * attempt);
      }
    }
  })();
  ready.catch(() => undefined);
  const readyInfo = ready.then(({ persistent }) => ({ persistent }));
  readyInfo.catch(() => undefined);

  async function send(method: RpcMethod, args: unknown[], bytes: Uint8Array | null, onProgress?: (p: Progress) => void) {
    const { worker } = await ready;
    if (fatal) throw fatal;
    if (!persistAsked && opts.persist && isWrite(method)) {
      persistAsked = true;
      void opts.persist().catch(() => false);
    }
    const id = nextId++;
    return new Promise<{ json?: string; bytes?: Uint8Array | null }>((resolve, reject) => {
      pending.set(id, { resolve, reject, onProgress });
      worker.postMessage({ type: 'call', id, method, args, bytes, progress: !!onProgress });
    });
  }

  return {
    kind: 'wasm',
    ready: readyInfo,
    async call(method, args, bytes, onProgress) {
      const r = await send(method, args, bytes, onProgress);
      return r.json === undefined || r.json === '' ? null : (JSON.parse(r.json) as unknown);
    },
    async callBytes(method, args, onProgress) {
      const r = await send(method, args, null, onProgress);
      return r.bytes && r.bytes.byteLength ? r.bytes : null;
    },
    async saveFile(bytes, filename, mime) {
      downloadBlob(bytes, filename, mime);
      return filename;
    },
  };
}

/** The browser defaults: a module worker, `navigator.locks`, `navigator.storage.persist`. */
export function createBrowserWasmTransport(spawn: () => WorkerLike): WasmTransport {
  const nav = typeof navigator !== 'undefined' ? navigator : undefined;
  return createWasmTransport({
    spawn,
    locks: (nav?.locks as LocksLike | undefined) ?? null,
    persist: nav?.storage?.persist ? () => nav.storage.persist() : null,
  });
}
