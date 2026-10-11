import { expect, test, type Page } from '@playwright/test';
import { importFixture } from './helpers';

async function browseSearch(page: Page, q: string) {
  const box = page.getByRole('searchbox', { name: 'Search cards' });
  await box.fill(q);
  await box.press('Enter');
}

test('(d) add a cloze note from the Add sheet and find it in Browse', async ({ page }) => {
  // Typing fast into a Cloze field once threw "Minified React error #185" (fixed; fast-typing.spec.ts).
  const pageErrors: string[] = [];
  page.on('pageerror', (e) => pageErrors.push(e.message));
  test.info().annotations.push({ type: 'page-errors', description: 'see below' });
  await importFixture(page);
  await page.getByRole('button', { name: 'Add A' }).click();
  const sheet = page.getByRole('dialog', { name: 'Add note' });
  await expect(sheet).toBeVisible();

  // switch the note type to Cloze
  await sheet.getByRole('button', { name: /^Note type / }).click();
  await page.getByRole('option', { name: 'Cloze', exact: true }).click();
  await expect(sheet.getByRole('button', { name: 'Note type Cloze' })).toBeVisible();

  const text = sheet.getByRole('textbox', { name: 'Text' });
  await text.click();
  await page.keyboard.type('The hippocampus binds an episode into one memory trace');
  // select the word "hippocampus" inside the field
  await text.evaluate((el) => {
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      const i = n.textContent!.indexOf('hippocampus');
      if (i >= 0) {
        const r = document.createRange();
        r.setStart(n, i);
        r.setEnd(n, i + 'hippocampus'.length);
        const sel = window.getSelection()!;
        sel.removeAllRanges();
        sel.addRange(r);
        return;
      }
    }
    throw new Error('text not found');
  });
  await page.keyboard.press('Control+Shift+C');
  await expect(text).toContainText('{{c1::hippocampus}}');

  // the live preview shows the cloze as [...]
  const preview = sheet.getByRole('region', { name: 'Preview' });
  await expect(preview.getByText('[...]').first()).toBeVisible();

  await sheet.getByRole('button', { name: 'Add', exact: true }).click();
  await expect(page.getByRole('region', { name: 'Notifications' }).getByText(/^Added\. 1 card created\./)).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(sheet).toHaveCount(0);

  await page.goto('/#/browse');
  await browseSearch(page, '"binds an episode"');
  const grid = page.getByRole('grid', { name: 'Search results' });
  await expect(page.getByRole('main').getByText(/^1 card$/).first()).toBeVisible();
  await expect(grid.getByRole('row').nth(1)).toContainText('hippocampus');
  const a = test.info().annotations.find((x) => x.type === 'page-errors')!;
  a.description = pageErrors.length ? pageErrors.map((m) => m.slice(0, 120)).join(' | ') : 'none';
  expect(pageErrors).toEqual([]);
});

test('(e) Browse: search, select all, add a tag in bulk, verify, undo', async ({ page }) => {
  await importFixture(page);
  await page.goto('/#/browse');
  await browseSearch(page, 'deck:Languages is:due');
  const main = page.getByRole('main');
  const status = main.getByText(/^\d+ cards$/).first();
  await expect(status).toBeVisible();
  const n = Number((await status.innerText()).match(/\d+/)![0]);
  expect(n).toBeGreaterThan(1);

  const grid = page.getByRole('grid', { name: 'Search results' });
  await grid.getByRole('row').nth(1).click();
  await grid.focus();
  await page.keyboard.press('Control+a');
  const bulk = page.getByRole('toolbar', { name: /./ }).filter({ has: page.getByRole('button', { name: 'Add tags' }) });
  await expect(bulk).toBeVisible();
  await expect(main.getByText(new RegExp(`${n} selected`)).first()).toBeVisible();

  await bulk.getByRole('button', { name: 'Add tags' }).click();
  const dialog = page.getByRole('dialog', { name: 'Add tags' });
  await dialog.getByRole('combobox', { name: 'Tags' }).fill('e2e-bulk');
  await dialog.getByRole('combobox', { name: 'Tags' }).press('Enter');
  await dialog.getByRole('button', { name: 'Add', exact: true }).click();
  const toasts = page.getByRole('region', { name: 'Notifications' });
  await expect(toasts.getByText(/^Add tags.*: \d+ cards? changed$/i).or(toasts.getByText(/\d+ cards? changed/))).toBeVisible();

  // every matching note carries the tag now (tag search lists all cards of those notes)
  await browseSearch(page, 'deck:Languages is:due tag:e2e-bulk');
  await expect(main.getByText(new RegExp(`^${n} cards$`)).first()).toBeVisible();
  await browseSearch(page, 'tag:e2e-bulk');
  await expect(main.getByText(/^\d+ cards?$/).first()).toBeVisible();
  const tagged = Number((await main.getByText(/^\d+ cards?$/).first().innerText()).match(/\d+/)![0]);
  expect(tagged).toBeGreaterThanOrEqual(n);

  // undo (Ctrl+Z outside text fields) removes it again
  await page.getByRole('searchbox', { name: 'Search cards' }).press('Escape');
  await page.keyboard.press('Control+z');
  await expect(toasts.getByText(/^Undone/)).toBeVisible();
  await browseSearch(page, 'deck:Languages');
  await browseSearch(page, 'tag:e2e-bulk');
  await expect(main.getByText(/^No cards$/).first()).toBeVisible();
});

