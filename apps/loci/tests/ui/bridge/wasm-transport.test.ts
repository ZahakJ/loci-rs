import { describe, expect, it, vi } from 'vitest';
import { createWasmTransport, LOCK_NAME, type FromWorker, type LocksLike, type ToWorker, type WorkerLike } from '../../../src/bridge/wasm';

/** A worker double: records calls and lets the test answer them. */
class FakeWorker implements WorkerLike {
  sent: ToWorker[] = [];
  private onMessage: ((e: MessageEvent<FromWorker>) => void) | null = null;
  private onError: ((e: ErrorEvent) => void) | null = null;
  terminated = false;
  postMessage(msg: ToWorker): void { this.sent.push(msg); }
  terminate(): void { this.terminated = true; }
  addEventListener(type: 'message' | 'error', cb: never): void {
    if (type === 'message') this.onMessage = cb; else this.onError = cb;
  }
  emit(m: FromWorker): void { this.onMessage?.({ data: m } as MessageEvent<FromWorker>); }
  crash(message: string): void { this.onError?.({ message } as ErrorEvent); }
}

const tick = () => new Promise((r) => setTimeout(r, 0));

function freeLocks(): LocksLike & { names: string[] } {
  const names: string[] = [];
  return { names, request: (name, _o, cb) => { names.push(name); return cb({ name }); } };
}

describe('wasm transport', () => {
  it('takes the collection lock, waits for ready and round-trips JSON, bytes and progress', async () => {
    const worker = new FakeWorker();
    const locks = freeLocks();
    const t = createWasmTransport({ spawn: () => worker, locks });
    expect(t.kind).toBe('wasm');
    const progress = vi.fn();
    const call = t.call('importPackage', [{ mode: 'merge' }], new Uint8Array([1, 2]), progress);
    await tick();
    expect(locks.names).toEqual([LOCK_NAME]);
    expect(worker.sent).toHaveLength(0);
    worker.emit({ type: 'ready', persistent: true });
    await expect(t.ready).resolves.toEqual({ persistent: true });
    await tick();
    const [msg] = worker.sent;
    expect(msg).toMatchObject({ type: 'call', method: 'importPackage', args: [{ mode: 'merge' }], progress: true });
    expect(Array.from(msg!.bytes!)).toEqual([1, 2]);
    worker.emit({ type: 'progress', id: msg!.id, progress: { phase: 'notes', done: 1, total: 2 } });
    worker.emit({ type: 'result', id: msg!.id, ok: true, json: '{"notesAdded":3}' });
    await expect(call).resolves.toEqual({ notesAdded: 3 });
    expect(progress).toHaveBeenCalledWith({ phase: 'notes', done: 1, total: 2 });

    const media = t.callBytes('getMedia', ['a.png']);
    await tick();
    worker.emit({ type: 'result', id: worker.sent[1]!.id, ok: true, bytes: new Uint8Array([9]) });
    expect(Array.from((await media)!)).toEqual([9]);
    const missing = t.callBytes('getMedia', ['b.png']);
    await tick();
    worker.emit({ type: 'result', id: worker.sent[2]!.id, ok: true });
    await expect(missing).resolves.toBeNull();
    const unit = t.call('renameDeck', [1, 'x'], null);
    await tick();
    worker.emit({ type: 'result', id: worker.sent[3]!.id, ok: true, json: 'null' });
    await expect(unit).resolves.toBeNull();
  });

  it('rejects with the engine error and asks for persistent storage once, on the first write', async () => {
    const worker = new FakeWorker();
    const persist = vi.fn(async () => true);
    const t = createWasmTransport({ spawn: () => worker, locks: null, persist });
    worker.emit({ type: 'ready', persistent: true });
    const read = t.call('deckTree', [], null);
    await tick();
    expect(persist).not.toHaveBeenCalled();
    worker.emit({ type: 'result', id: worker.sent[0]!.id, ok: false, error: { code: 'invalid', message: 'nope' } });
    await expect(read).rejects.toEqual({ code: 'invalid', message: 'nope' });
    void t.call('answerCard', [1, 3, 100], null);
    void t.call('answerCard', [1, 3, 100], null);
    await tick();
    expect(persist).toHaveBeenCalledTimes(1);
  });

  it('refuses every call when another tab holds the collection, without starting the engine', async () => {
    const spawn = vi.fn(() => new FakeWorker());
    const locks: LocksLike = { request: (_n, _o, cb) => cb(null) };
    const t = createWasmTransport({ spawn, locks });
    await expect(t.call('open', [], null)).rejects.toMatchObject({ code: 'busy', message: 'Loci is open in another tab', detail: 'other-tab' });
    expect(spawn).not.toHaveBeenCalled();
  });

  it('fails pending and later calls when the worker dies', async () => {
    const worker = new FakeWorker();
    const t = createWasmTransport({ spawn: () => worker, locks: null });
    worker.emit({ type: 'ready', persistent: false });
    await expect(t.ready).resolves.toEqual({ persistent: false });
    const pending = t.call('stats', [null], null);
    await tick();
    worker.crash('boom');
    await expect(pending).rejects.toMatchObject({ code: 'internal' });
    await expect(t.call('today', [], null)).rejects.toMatchObject({ code: 'internal' });
  });

  it('starts a fresh worker while the storage is still held by a closing tab', async () => {
    const workers: FakeWorker[] = [];
    const t = createWasmTransport({ spawn: () => { const w = new FakeWorker(); workers.push(w); return w; }, locks: null, delay: async () => undefined });
    const call = t.call('open', [], null);
    await tick();
    workers[0]!.emit({ type: 'fatal', error: { code: 'busy', message: 'held', detail: 'storage-busy' } });
    await tick();
    expect(workers).toHaveLength(2);
    expect(workers[0]!.terminated).toBe(true);
    workers[1]!.emit({ type: 'ready', persistent: true });
    await tick();
    workers[1]!.emit({ type: 'result', id: workers[1]!.sent[0]!.id, ok: true, json: '{"ok":true}' });
    await expect(call).resolves.toEqual({ ok: true });
  });

  it('reports a worker that cannot start the engine', async () => {
    const worker = new FakeWorker();
    const t = createWasmTransport({ spawn: () => worker, locks: null });
    const call = t.call('open', [], null);
    await tick();
    worker.emit({ type: 'fatal', error: { code: 'io', message: 'no storage' } });
    await expect(call).rejects.toEqual({ code: 'io', message: 'no storage' });
  });
});
