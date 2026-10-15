/** `@loci-web-spawn` in builds without the WebAssembly engine (desktop, Android, dev): never called. */
import type { WorkerLike } from './wasm';

export function spawnEngineWorker(): WorkerLike {
  throw new Error('This build does not include the browser engine');
}
