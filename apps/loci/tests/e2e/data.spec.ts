import { expect, test } from '@playwright/test';
import { readFile, mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { BlobReader, Uint8ArrayWriter, ZipReader, configure } from '@zip.js/zip.js';
import { FIXTURE, importFixture } from './helpers';

configure({ useWebWorkers: false });

const STATS_PANELS = [
  'Worth knowing', 'Today', 'Forecast', 'True retention', 'Reviews per day', 'Activity', 'Intervals',
  'Stability', 'Difficulty', 'Time of day', 'Answer buttons', 'Card counts', 'Momentum',
];

async function unzip(bytes: Uint8Array): Promise<Map<string, Uint8Array>> {
  const reader = new ZipReader(new BlobReader(new Blob([bytes as Uint8Array<ArrayBuffer>])));
  const out = new Map<string, Uint8Array>();
  for (const e of await reader.getEntries()) {
    if (!e.directory && 'getData' in e && e.getData) out.set(e.filename, await e.getData(new Uint8ArrayWriter()));
  }
  await reader.close();
  return out;
}

/** Opens a SQLite image from bytes via a temp file (node:sqlite has no in-memory deserialize). */
async function withDb<T>(bytes: Uint8Array, fn: (db: DatabaseSync) => T): Promise<T> {
  const dir = await mkdtemp(path.join(tmpdir(), 'loci-e2e-'));
  const file = path.join(dir, 'c.db');
  try {
    await writeFile(file, bytes);
    const db = new DatabaseSync(file, { readOnly: true });
    try { return fn(db); } finally { db.close(); }
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}

/** Notes with at least one card in `deck` (filtered cards count for their home deck), straight from the fixture. */
async function fixtureNotesInDeck(deck: string): Promise<number> {
  const files = await unzip(new Uint8Array(await readFile(FIXTURE)));
  const col = files.get('collection.anki21') ?? files.get('collection.anki2')!;
  return withDb(col, (db) => {
    const decks = JSON.parse((db.prepare('select decks from col').get() as { decks: string }).decks) as Record<string, { id: number; name: string }>;
    const ids = Object.values(decks).filter((d) => d.name === deck || d.name.startsWith(`${deck}::`)).map((d) => d.id);
    const row = db.prepare(`select count(distinct nid) n from cards where (case when odid != 0 then odid else did end) in (${ids.join(',')})`).get() as { n: number };
    return row.n;
  });
}

test('(f) Stats renders every panel and the insights strip', async ({ page }) => {
  await importFixture(page);
  await page.getByRole('link', { name: 'Stats S' }).click();
  await expect(page.getByRole('heading', { level: 1, name: 'Statistics' })).toBeVisible();
  for (const h of STATS_PANELS) await expect(page.getByRole('heading', { level: 2, name: h, exact: true })).toBeVisible();
  const insights = page.getByRole('region', { name: 'Worth knowing' });
  expect(await insights.getByRole('listitem').count()).toBeGreaterThan(0);
  await expect(insights.getByRole('link', { name: 'Plan a catch-up' })).toBeVisible();
});

test('(g) exporting a deck downloads an .apkg Anki can read', async ({ page }) => {
  await importFixture(page);
  const expected = await fixtureNotesInDeck('History');
  expect(expected).toBeGreaterThan(0);
  await page.getByRole('button', { name: 'Actions for History' }).click();
  await page.getByRole('menuitem', { name: 'Export as .apkg' }).click();
  const dialog = page.getByRole('dialog', { name: 'Export “History”' });
  await expect(dialog).toBeVisible();
  const [download] = await Promise.all([
    page.waitForEvent('download'),
    dialog.getByRole('button', { name: 'Export .apkg' }).click(),
  ]);
  await expect(dialog).toHaveCount(0);
  expect(download.suggestedFilename()).toMatch(/\.apkg$/);
  const bytes = new Uint8Array(await readFile(await download.path()));
  const files = await unzip(bytes);
  expect([...files.keys()]).toContain('collection.anki21');
  const counts = await withDb(files.get('collection.anki21')!, (db) => ({
    notes: (db.prepare('select count(*) n from notes').get() as { n: number }).n,
    cards: (db.prepare('select count(*) n from cards').get() as { n: number }).n,
    revlog: (db.prepare('select count(*) n from revlog').get() as { n: number }).n,
  }));
  expect(counts.notes).toBe(expected);
  expect(counts.cards).toBe(136); // History's cards in the fixture (palace.expected.json)
  expect(counts.revlog).toBeGreaterThan(0);
});

test('(h) backups: back up now, see it listed, restore it', async ({ page }) => {
  await importFixture(page);
  await page.goto('/#/backups');
  await expect(page.getByRole('heading', { level: 1, name: 'Backups' })).toBeVisible();
  await page.getByRole('button', { name: 'Back up now' }).click();
  const toasts = page.getByRole('region', { name: 'Notifications' });
  await expect(toasts.getByText(/^Backed up \(/)).toBeVisible({ timeout: 30_000 });
  const table = page.getByRole('table', { name: 'Backups, newest first' });
  const manual = table.getByRole('row').filter({ has: page.getByRole('cell', { name: 'Manual', exact: true }) }).first();
  await expect(manual).toBeVisible();
  await expect(manual.getByRole('cell', { name: '590 cards' })).toBeVisible();

  // change something, then restore the manual backup and see the change gone
  await page.goto('/#/');
  await page.getByRole('button', { name: 'New deck' }).click();
  const create = page.getByRole('dialog', { name: 'New deck' });
  await create.getByRole('textbox', { name: 'Name' }).fill('Throwaway e2e');
  await create.getByRole('button', { name: 'Create deck' }).click();
  await expect(page.getByRole('list', { name: 'Decks' }).getByRole('link', { name: /^Throwaway e2e: / })).toBeVisible();

  await page.goto('/#/backups');
  await table.getByRole('row').filter({ has: page.getByRole('cell', { name: 'Manual', exact: true }) }).first().getByRole('button', { name: /^Restore the backup from / }).click();
  const confirm = page.getByRole('dialog', { name: /^Restore the backup from .+\?$/ });
  await expect(confirm).toBeVisible();
  await confirm.getByRole('button', { name: 'Restore', exact: true }).click();
  await expect(toasts.getByText(/^Restored\./)).toBeVisible({ timeout: 60_000 });
  await expect(table.getByRole('cell', { name: 'Before restore', exact: true })).toBeVisible();

  await page.goto('/#/');
  await expect(page.getByRole('list', { name: 'Decks' }).getByRole('link', { name: /^History: / })).toBeVisible();
  await expect(page.getByRole('list', { name: 'Decks' }).getByRole('link', { name: /^Throwaway e2e: / })).toHaveCount(0);
});
