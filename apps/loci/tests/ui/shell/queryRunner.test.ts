import { describe, expect, it, vi } from 'vitest';
import { QueryRunner, shouldRefetch } from '@ui/hooks/queryRunner';

const deferred = <T,>() => { let resolve!: (v: T) => void, reject!: (e: unknown) => void; const promise = new Promise<T>((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; };

describe('shouldRefetch', () => {
  it('matches watched change keys only', () => {
    expect(shouldRefetch({ type: 'changed', what: ['decks'] }, ['decks', 'cards'])).toBe(true);
    expect(shouldRefetch({ type: 'changed', what: ['settings'] }, ['decks'])).toBe(false);
    expect(shouldRefetch({ type: 'changed', what: ['settings'] }, 'all')).toBe(true);
    expect(shouldRefetch({ type: 'changed', what: ['decks'] }, 'none')).toBe(false);
    expect(shouldRefetch({ type: 'progress', op: 'import', progress: { phase: 'x', done: 1, total: 2 } }, 'all')).toBe(false);
  });
});

describe('QueryRunner', () => {
  it('loads, then keeps data while refetching', async () => {
    let n = 0;
    const r = new QueryRunner(async () => ++n);
    expect(r.getState()).toMatchObject({ loading: true, data: undefined });
    await r.run();
    expect(r.getState()).toMatchObject({ loading: false, fetching: false, data: 1 });
    const p = r.run();
    expect(r.getState()).toMatchObject({ loading: false, fetching: true, data: 1 });
    await p;
    expect(r.getState().data).toBe(2);
  });

  it('drops stale responses (latest request wins)', async () => {
    const a = deferred<string>(), b = deferred<string>();
    const queue = [a.promise, b.promise];
    const r = new QueryRunner(() => queue.shift()!);
    const first = r.run();
    const second = r.run();
    b.resolve('new');
    await second;
    a.resolve('old');
    await first;
    expect(r.getState().data).toBe('new');
  });

  it('records errors and clears them on success', async () => {
    let fail = true;
    const r = new QueryRunner(async () => { if (fail) throw new Error('boom'); return 'ok'; });
    await r.run();
    expect(r.getState().error).toBeInstanceOf(Error);
    expect(r.getState().loading).toBe(false);
    fail = false;
    await r.run();
    expect(r.getState()).toMatchObject({ error: undefined, data: 'ok' });
  });

  it('coalesces scheduled refetches', async () => {
    vi.useFakeTimers();
    const fn = vi.fn(async () => 1);
    const r = new QueryRunner(fn, 30);
    r.schedule(); r.schedule(); r.schedule();
    await vi.advanceTimersByTimeAsync(40);
    expect(fn).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
  });

  it('notifies subscribers and supports optimistic updates', async () => {
    const r = new QueryRunner(async () => [1]);
    const cb = vi.fn();
    const off = r.subscribe(cb);
    await r.run();
    r.setData((prev) => [...(prev ?? []), 2]);
    expect(r.getState().data).toEqual([1, 2]);
    expect(cb).toHaveBeenCalled();
    off();
    const calls = cb.mock.calls.length;
    r.setData([3]);
    expect(cb.mock.calls.length).toBe(calls);
  });

  it('dispose ignores in-flight results', async () => {
    const d = deferred<number>();
    const r = new QueryRunner(() => d.promise);
    const p = r.run();
    r.dispose();
    d.resolve(5);
    await p;
    expect(r.getState().data).toBeUndefined();
  });
});
