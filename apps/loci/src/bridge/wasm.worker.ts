/// <reference lib="webworker" />
/**
 * The engine's worker in the browser build: loads `loci-web` (the Rust engine as WebAssembly),
 * opens the collection in the origin-private file system and answers `wasm.ts` calls one at a
 * time. A panic inside the engine leaves the module unusable; the worker then reports `fatal` and
 * refuses further calls (a reload starts a fresh one; SQLite rolls back anything unfinished).
 */
import init, { WebEngine } from '@loci-web';
import type { SerializedError } from './protocol';
import type { FromWorker, ToWorker } from './wasm';

const scope = self as unknown as DedicatedWorkerGlobalScope;
const post = (m: FromWorker, transfer: Transferable[] = []) => scope.postMessage(m, transfer);

function toError(e: unknown): SerializedError {
  if (typeof e === 'string') {
    try {
      const parsed = JSON.parse(e) as SerializedError;
      if (parsed && typeof parsed.code === 'string') return parsed;
    } catch { /* not JSON */ }
    return { code: 'internal', message: e };
  }
  if (e instanceof Error) return { code: 'internal', message: e.message };
  return { code: 'internal', message: String(e) };
}

let broken: SerializedError | null = null;

/** OPFS with synchronous access handles (dedicated workers in current browsers). */
function opfsAvailable(): boolean {
  return typeof navigator !== 'undefined' && typeof navigator.storage?.getDirectory === 'function'
    && typeof FileSystemFileHandle !== 'undefined' && 'createSyncAccessHandle' in FileSystemFileHandle.prototype;
}

const engine: Promise<WebEngine> = (async () => {
  await init();
  const opened = await WebEngine.open(opfsAvailable());
  post({ type: 'ready', persistent: opened.persistent });
  return opened;
})();
engine.catch((e: unknown) => post({ type: 'fatal', error: toError(e) }));

scope.addEventListener('message', (ev: MessageEvent<ToWorker>) => {
  const m = ev.data;
  if (m.type !== 'call') return;
  void engine.then((eng) => {
    if (broken) { post({ type: 'result', id: m.id, ok: false, error: broken }); return; }
    const onProgress = m.progress
      ? (p: { phase: string; done: number; total: number }) => post({ type: 'progress', id: m.id, progress: p })
      : undefined;
    let out: unknown;
    try {
      out = eng.call(m.method, JSON.stringify(m.args), m.bytes ?? undefined, onProgress);
    } catch (e) {
      const error = toError(e);
      // Engine errors arrive as JSON strings; anything else (a trap) means the module is broken.
      if (typeof e !== 'string') {
        broken = { code: 'internal', message: `The engine stopped: ${error.message}. Reload the page.` };
        post({ type: 'fatal', error: broken });
      }
      post({ type: 'result', id: m.id, ok: false, error });
      return;
    }
    if (out instanceof Uint8Array) post({ type: 'result', id: m.id, ok: true, bytes: out }, [out.buffer]);
    else if (typeof out === 'string') post({ type: 'result', id: m.id, ok: true, json: out });
    else post({ type: 'result', id: m.id, ok: true, bytes: null });
  }, () => post({ type: 'result', id: m.id, ok: false, error: { code: 'internal', message: 'The engine did not start' } }));
});
