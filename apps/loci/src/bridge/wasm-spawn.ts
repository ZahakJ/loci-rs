/** Starts the engine worker of the browser build (`@loci-web-spawn`; other builds get `wasm-spawn-stub.ts`). */
import type { WorkerLike } from './wasm';

export function spawnEngineWorker(): WorkerLike {
  return new Worker(new URL('./wasm.worker.ts', import.meta.url), { type: 'module', name: 'loci-engine' });
}
