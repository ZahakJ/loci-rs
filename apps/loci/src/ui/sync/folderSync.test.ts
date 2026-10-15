import { describe, expect, it } from 'vitest';
import type { SyncMediaRef, SyncReport, SyncStatus } from '@core/types';
import { describeSync, syncFileName } from './describe';
import { deviceOfFile, memoryFolder, syncFolder, type SyncApi } from './folderSync';

const enc = (s: string) => new TextEncoder().encode(s);
const dec = (b: Uint8Array) => new TextDecoder().decode(b);

/** A stand-in engine: a set of rows, a collection id that converges on the smaller, media by name. */
class FakeDevice implements SyncApi {
  rows = new Set<string>();
  media = new Map<string, { sha1: string; bytes: Uint8Array | null }>();
  merged: string[] = [];
  constructor(public deviceId: string, public collectionId: string, public formerDevices: string[] = []) {}
  async syncStatus(): Promise<SyncStatus> {
    return { enabled: true, deviceId: this.deviceId, collectionId: this.collectionId, folder: '/f', lastSync: null, peers: [], formerDevices: this.formerDevices };
  }
  async exportSyncBundle(): Promise<Uint8Array> {
    const media = [...this.media].map(([name, m]) => ({ name, sha1: m.sha1 }));
    return enc(JSON.stringify({ device: this.deviceId, collection: this.collectionId, rows: [...this.rows], media }));
  }
  async mergeSyncBundle(bytes: Uint8Array): Promise<SyncReport> {
    const b = JSON.parse(dec(bytes)) as { device: string; collection: string; rows: string[]; media: SyncMediaRef[] };
    let rowsIn = 0;
    for (const r of b.rows) if (!this.rows.has(r)) { this.rows.add(r); rowsIn++; }
    for (const m of b.media) if (!this.media.has(m.name)) this.media.set(m.name, { sha1: m.sha1, bytes: null });
    if (b.collection < this.collectionId) this.collectionId = b.collection;
    this.merged.push(b.device);
    return { fromDevice: b.device, rowsIn, conflictsResolved: 0, reviewsAdded: 0, mediaAdded: 0, mediaNeeded: [], complete: true };
  }
  async syncMediaState() {
    const all = [...this.media].map(([name, m]) => ({ name, sha1: m.sha1, has: !!m.bytes }));
    return { have: all.filter((m) => m.has).map(({ name, sha1 }) => ({ name, sha1 })), need: all.filter((m) => !m.has).map(({ name, sha1 }) => ({ name, sha1 })) };
  }
  async addSyncMedia(sha1: string, bytes: Uint8Array): Promise<number> {
    let n = 0;
    for (const m of this.media.values()) if (m.sha1 === sha1 && !m.bytes) { m.bytes = bytes; n++; }
    return n;
  }
  async getMedia(name: string): Promise<Uint8Array | null> { return this.media.get(name)?.bytes ?? null; }
}

const A = 'aaaaaaaaaaaaaaaa';
const B = 'bbbbbbbbbbbbbbbb';
const C = 'cccccccccccccccc';
const SHA = '0123456789abcdef0123456789abcdef01234567';

describe('folder sync', () => {
  it('each device writes only its own file and reads everyone else\'s', async () => {
    const folder = memoryFolder();
    const a = new FakeDevice(A, 'col-1');
    const b = new FakeDevice(B, 'col-1');
    a.rows.add('note 1');
    b.rows.add('note 2');
    await syncFolder(a, folder);
    expect([...folder.files.keys()]).toEqual([`loci-sync/col-1/${A}.loci-sync`]);
    const rb = await syncFolder(b, folder);
    expect(rb).toMatchObject({ devices: 1, rowsIn: 1 });
    expect(b.rows).toEqual(new Set(['note 1', 'note 2']));
    const ra = await syncFolder(a, folder);
    expect(ra.rowsIn).toBe(1);
    expect(a.merged).toEqual([B]);
    expect([...folder.files.keys()].sort()).toEqual([`loci-sync/col-1/${A}.loci-sync`, `loci-sync/col-1/${B}.loci-sync`]);
  });

  it('ignores conflict copies, removes its own stale files and joins the folder\'s collection', async () => {
    const folder = memoryFolder();
    const a = new FakeDevice(A, 'col-1');
    a.rows.add('x');
    await syncFolder(a, folder);
    folder.files.set(`loci-sync/col-1/${A}.sync-conflict-20260101.loci-sync`, enc('garbage'));
    folder.files.set('loci-sync/col-1/notes.txt', enc('hello'));
    // C used to be D (before a restore) and started on its own collection id.
    const D = 'dddddddddddddddd';
    folder.files.set(`loci-sync/col-1/${D}.loci-sync`, enc('stale'));
    const c = new FakeDevice(C, 'col-9', [D]);
    folder.files.set(`loci-sync/col-9/${C}.loci-sync`, enc('old'));
    const r = await syncFolder(c, folder);
    expect(r).toMatchObject({ devices: 1, skipped: 0 });
    expect(c.collectionId).toBe('col-1');
    expect(folder.files.has(`loci-sync/col-1/${C}.loci-sync`)).toBe(true);
    expect(folder.files.has(`loci-sync/col-9/${C}.loci-sync`)).toBe(false);
    expect(folder.files.has(`loci-sync/col-1/${D}.loci-sync`)).toBe(false);
    expect(folder.files.has(`loci-sync/col-1/${A}.sync-conflict-20260101.loci-sync`)).toBe(true);
  });

  it('skips a file it cannot merge and tries again next time', async () => {
    const folder = memoryFolder();
    folder.files.set(`loci-sync/col-1/${B}.loci-sync`, enc('half-written'));
    const a = new FakeDevice(A, 'col-1');
    const r = await syncFolder(a, folder);
    expect(r).toMatchObject({ devices: 0, skipped: 1 });
    expect(folder.files.has(`loci-sync/col-1/${A}.loci-sync`)).toBe(true);
  });

  it('copies media into the folder by checksum and takes what it lacks', async () => {
    const folder = memoryFolder();
    const a = new FakeDevice(A, 'col-1');
    a.media.set('cat.png', { sha1: SHA, bytes: enc('meow') });
    const ra = await syncFolder(a, folder);
    expect(ra.mediaOut).toBe(1);
    expect(dec(folder.files.get(`loci-sync/col-1/media/${SHA}`)!)).toBe('meow');
    expect((await syncFolder(a, folder)).mediaOut).toBe(0);
    const b = new FakeDevice(B, 'col-1');
    const rb = await syncFolder(b, folder);
    expect(rb.mediaIn).toBe(1);
    expect(dec((await b.getMedia('cat.png'))!)).toBe('meow');
  });
});

describe('sync helpers', () => {
  it('recognises device files only', () => {
    expect(deviceOfFile(`${A}.loci-sync`)).toBe(A);
    expect(deviceOfFile(`${A}.sync-conflict-1.loci-sync`)).toBeNull();
    expect(deviceOfFile('notes.loci-sync')).toBeNull();
  });

  it('names files and describes results', () => {
    expect(syncFileName(new Date(2026, 9, 14))).toBe('loci-2026-10-14.loci-sync');
    expect(describeSync({ rowsIn: 0, reviewsAdded: 0 })).toBe('already up to date');
    expect(describeSync({ rowsIn: 3, reviewsAdded: 1, mediaIn: 2 })).toBe('3 changes · 1 review · 2 media files');
    expect(describeSync({ rowsIn: 1, reviewsAdded: 0, complete: false })).toBe('1 change · sync once more to finish');
  });
});
