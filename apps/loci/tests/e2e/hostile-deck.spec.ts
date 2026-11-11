import { expect, test, type Page } from '@playwright/test';
import { BlobReader, BlobWriter, Uint8ArrayReader, Uint8ArrayWriter, ZipReader, ZipWriter, configure } from '@zip.js/zip.js';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { FIXTURE, watchErrors } from './helpers';

configure({ useWebWorkers: false });

// A shared deck is untrusted input. This one carries field HTML that would run script if it
// reached the page unsanitised; opening the note in the editor and studying it must run none.
const MARK = 'hostilemarker';
const FRONT = [
  `${MARK} front`,
  `<img src="https://invalid.example/x.png" onerror="window.__lociPwned='img'">`,
  `<details open ontoggle="window.__lociPwned='details'"><summary>s</summary>d</details>`,
  `<a href="javascript:window.__lociPwned='href'">link</a>`,
  `<script>window.__lociPwned='script'</script>`,
  `<iframe srcdoc="<script>parent.__lociPwned='iframe'</script>"></iframe>`,
  `<svg><animate onbegin="window.__lociPwned='svg'" attributeName="x" dur="1s"></animate></svg>`,
  `<span id="root" style="color:red">clobber</span>`,
].join(' ');
const BACK = `${MARK} back <img src=x onerror="window.__lociPwned='back'">`;

/** palace-legacy2.apkg with one Basic note's fields replaced by FRONT and BACK. */
async function hostileDeck(dir: string): Promise<string> {
  const reader = new ZipReader(new BlobReader(new Blob([readFileSync(FIXTURE)])));
  const writer = new ZipWriter(new BlobWriter('application/zip'));
  for (const entry of await reader.getEntries()) {
    if (entry.directory || !entry.getData) continue;
    let bytes = await entry.getData(new Uint8ArrayWriter());
    if (entry.filename === 'collection.anki21') {
      const file = path.join(dir, 'collection.anki21');
      writeFileSync(file, bytes);
      const db = new DatabaseSync(file);
      const row = db.prepare("SELECT id FROM notes WHERE mid = 1684000000001 ORDER BY id LIMIT 1").get() as { id: number };
      db.prepare('UPDATE notes SET flds = ?, sfld = ? WHERE id = ?').run(`${FRONT}\x1f${BACK}`, `${MARK} front`, row.id);
      db.close();
      bytes = readFileSync(file);
    }
    await writer.add(entry.filename, new Uint8ArrayReader(bytes));
  }
  await reader.close();
  const out = path.join(dir, 'hostile.apkg');
  writeFileSync(out, Buffer.from(await (await writer.close()).arrayBuffer()));
  return out;
}

async function importDeck(page: Page, file: string): Promise<void> {
  await page.goto('/#/import');
  await expect(page.getByRole('heading', { level: 1, name: 'Import' })).toBeVisible();
  await page.locator('input[type=file]').setInputFiles(file);
  await page.getByRole('button', { name: 'Import 402 new notes' }).click();
  await expect(page.getByRole('heading', { name: '402 notes imported' })).toBeVisible({ timeout: 60_000 });
}

const pwned = (page: Page) => page.evaluate(() => (window as unknown as { __lociPwned?: string }).__lociPwned ?? null);

test('field HTML from an imported deck runs no script in the editor', async ({ page }) => {
  const dir = mkdtempSync(path.join(tmpdir(), 'loci-hostile-'));
  try {
    const errors = watchErrors(page);
    await importDeck(page, await hostileDeck(dir));

    await page.goto('/#/browse');
    const box = page.getByRole('searchbox', { name: 'Search cards' });
    await box.fill(MARK);
    await box.press('Enter');
    const grid = page.getByRole('grid', { name: 'Search results' });
    await expect(page.getByRole('main').getByText(/^1 card$/).first()).toBeVisible();
    await grid.getByRole('row').nth(1).click();

    const front = page.getByRole('textbox', { name: 'Front' });
    const back = page.getByRole('textbox', { name: 'Back' });
    await expect(front).toContainText(`${MARK} front`);
    await expect(back).toContainText(`${MARK} back`);
    await page.waitForTimeout(1500);

    // The sanitiser removed what would run, whether or not the page's CSP would also stop it.
    for (const field of [front, back]) {
      const found = await field.evaluate((el) => ({
        handlers: Array.from(el.querySelectorAll('*')).flatMap((n) => n.getAttributeNames().filter((a) => a.startsWith('on'))),
        tags: Array.from(el.querySelectorAll('script, iframe, animate, button, style')).map((n) => n.tagName),
        jsLinks: Array.from(el.querySelectorAll('a[href]')).filter((a) => /^\s*javascript:/i.test(a.getAttribute('href')!)).length,
        ids: el.querySelectorAll('[id]').length,
      }));
      expect(found).toEqual({ handlers: [], tags: [], jsLinks: 0, ids: 0 });
    }
    expect(await page.evaluate(() => document.querySelectorAll('#root').length)).toBe(1);
    expect(await pwned(page)).toBeNull();

    // The card preview renders the same note through the card sanitiser.
    await page.keyboard.press('Escape');
    expect(await pwned(page)).toBeNull();
    expect(errors.filter((e) => !/invalid\.example|ERR_NAME_NOT_RESOLVED|Failed to load resource/.test(e))).toEqual([]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
