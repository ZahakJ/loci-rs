import { expect, test } from '@playwright/test';
import { importFixture } from './helpers';

test('Sync: a sync file saved on one device brings another up to date', async ({ browser }) => {
  // Two browser contexts are two devices (two collections behind `loci serve --multi-session`).
  const desk = await (await browser.newContext()).newPage();
  await importFixture(desk);
  await desk.goto('/#/settings?section=sync');
  const deskSync = desk.locator('#settings-sync');
  await expect(deskSync.getByRole('heading', { name: 'Sync' })).toBeVisible();
  const [download] = await Promise.all([
    desk.waitForEvent('download'),
    deskSync.getByRole('button', { name: 'Save', exact: true }).click(),
  ]);
  expect(download.suggestedFilename()).toMatch(/^loci-\d{4}-\d{2}-\d{2}\.loci-sync$/);
  const file = await download.path();

  const phone = await (await browser.newContext({ viewport: { width: 390, height: 844 } })).newPage();
  await phone.goto('/#/settings?section=sync');
  const phoneSync = phone.locator('#settings-sync');
  await expect(phoneSync.getByRole('button', { name: 'Open', exact: true })).toBeVisible();
  await phoneSync.locator('input[type=file]').setInputFiles(file);
  await expect(phone.getByText(/^Synced: [\d,]+ changes · [\d,]+ reviews/)).toBeVisible({ timeout: 60_000 });

  await phone.goto('/#/');
  await expect(phone.getByRole('heading', { level: 2, name: /to clear today/ })).toBeVisible();

  // Opening the same file again changes nothing.
  await phone.goto('/#/settings?section=sync');
  await phone.locator('#settings-sync input[type=file]').setInputFiles(file);
  await expect(phone.getByText('Synced: already up to date')).toBeVisible({ timeout: 30_000 });
});
