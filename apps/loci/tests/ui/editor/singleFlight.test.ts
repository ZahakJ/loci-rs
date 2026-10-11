import { describe, expect, it } from 'vitest';
import { singleFlight } from '@ui/views/edit/singleFlight';

describe('singleFlight (Add sheet submit guard, V-F01)', () => {
  it('ignores calls while the first one is in flight, then allows the next', async () => {
    let calls = 0;
    let release!: () => void;
    const add = singleFlight(() => { calls++; return new Promise<void>((r) => { release = r; }); });
    const first = add();
    // a second Ctrl+Enter handler / key repeat / double click in the same tick
    const second = add();
    const third = add();
    expect(add.pending).toBe(true);
    expect(await second).toBe(false);
    expect(await third).toBe(false);
    release();
    expect(await first).toBe(true);
    expect(calls).toBe(1);
    expect(add.pending).toBe(false);
    const again = add();
    release();
    expect(await again).toBe(true);
    expect(calls).toBe(2);
  });

  it('releases the guard when the action fails', async () => {
    let calls = 0;
    const add = singleFlight(async () => { calls++; throw new Error('boom'); });
    await expect(add()).rejects.toThrow('boom');
    expect(add.pending).toBe(false);
    await expect(add()).rejects.toThrow('boom');
    expect(calls).toBe(2);
  });

  it('passes arguments through', async () => {
    const seen: string[] = [];
    const run = singleFlight(async (kind: 'submit' | 'save') => { seen.push(kind); });
    await run('save');
    await run('submit');
    expect(seen).toEqual(['save', 'submit']);
  });
});
