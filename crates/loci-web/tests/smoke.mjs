// Smoke test of the WebAssembly engine in Node (in-memory VFS; OPFS exists only in browsers):
// open a collection, import the fixture package, study and answer a card, read stats, back up,
// restore, sync through a sync file, and check the error path.
//
//   node crates/loci-web/tests/smoke.mjs <wasm-bindgen --target web output dir>
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const pkg = resolve(process.argv[2] ?? 'apps/loci/wasm');
const root = resolve(import.meta.dirname, '../../..');
const { initSync, WebEngine } = await import(pathToFileURL(join(pkg, 'loci_web.js')).href);
initSync({ module: readFileSync(join(pkg, 'loci_web_bg.wasm')) });

const engine = await WebEngine.open(false);
assert.equal(engine.persistent, false);

const call = (method, args = [], bytes, onProgress) => {
  const out = engine.call(method, JSON.stringify(args), bytes, onProgress);
  return typeof out === 'string' ? JSON.parse(out) : out;
};

const report = call('open');
assert.equal(report.ok, true, 'integrity on open');
assert.equal(call('isEmpty'), true);

const expected = JSON.parse(readFileSync(join(root, 'fixtures/palace.expected.json'), 'utf8'));
const apkg = new Uint8Array(readFileSync(join(root, 'fixtures/palace-latest.apkg')));
const preview = call('previewImport', [], apkg);
assert.ok(preview, 'preview');
const phases = new Set();
const imported = call('importPackage', [{ mode: 'merge', keepScheduling: true }], apkg, (p) => phases.add(p.phase));
assert.equal(imported.notesAdded, expected.totals.notes, 'notes imported');
assert.equal(imported.cardsAdded, expected.totals.cards, 'cards imported');
assert.ok(phases.size > 0, 'progress reported');
assert.equal(call('isEmpty'), false);

const first = call('startStudy', [null]);
assert.ok(first && first.card, 'a card to study');
const answer = call('answerCard', [first.card.id, 3, 4000]);
assert.ok(answer.undoLabel, 'answer is undoable');
const stats = call('stats', [null]);
assert.equal(stats.today.studied, 1, 'stats count the answer');

const media = call('checkMedia');
assert.ok(media, 'media check');
const tree = call('deckTree');
assert.ok(tree.children.length > 0, 'deck tree');

const backup = call('backupNow');
assert.ok(backup.bytes > 0, 'backup written to the blob store');
assert.equal(call('listBackups').length, 1);
call('answerCard', [answer.next?.card.id ?? first.card.id, 3, 4000]);
call('restoreBackup', [backup.name]);
assert.equal(call('stats', [null]).today.studied, 1, 'restore brings back the backed-up state');

const full = call('exportFullBackup');
assert.ok(full instanceof Uint8Array && full.byteLength > 1000, 'full backup bytes');

// Sync: this collection's sync file brings everything back after a reset.
const syncFile = call('exportSyncBundle');
assert.ok(syncFile instanceof Uint8Array && syncFile.byteLength > 1000, 'sync file bytes');
const notesBefore = call('searchIds', ['', 'notes']).length;
call('resetCollection');
assert.equal(call('isEmpty'), true);
const merged = call('mergeSyncBundle', [], syncFile);
assert.ok(merged.rowsIn > 0 && merged.complete && merged.mediaNeeded.length === 0, 'sync merge');
assert.equal(call('searchIds', ['', 'notes']).length, notesBefore, 'notes back from the sync file');
assert.equal(call('mergeSyncBundle', [], syncFile).rowsIn, 0, 'merging twice changes nothing');

assert.throws(() => engine.call('getDeck', JSON.stringify(['x'])), (e) => JSON.parse(e).code === 'invalid');
console.log(`wasm smoke: ok (${imported.notesAdded} notes, ${imported.cardsAdded} cards, backup ${backup.bytes} bytes)`);
