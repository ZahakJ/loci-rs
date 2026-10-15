import { expect, test } from '@playwright/test';
import { pickFixture } from '../e2e/helpers';

test('the browser build saves a sync file and another browser brings itself up to date from it', async ({ browser }) => {
  const one = await (await browser.newContext()).newPage();
  await one.goto('./#/import');
  await pickFixture(one);
  await one.getByRole('button', { name: 'Import 402 new notes' }).click();
  await expect(one.getByRole('heading', { name: '402 notes imported' })).toBeVisible({ timeout: 60_000 });

  await one.goto('./#/settings?section=sync');
  const sync = one.locator('#settings-sync');
  // Chromium can open folders: the sync folder row is offered.
  await expect(sync.getByRole('button', { name: 'Choose folder' })).toBeVisible();
  const [download] = await Promise.all([one.waitForEvent('download'), sync.getByRole('button', { name: 'Save', exact: true }).click()]);
  const file = await download.path();

  const two = await (await browser.newContext()).newPage();
  await two.goto('./#/settings?section=sync');
  await two.locator('#settings-sync input[type=file]').setInputFiles(file);
  await expect(two.getByText(/^Synced: [\d,]+ changes · [\d,]+ reviews/)).toBeVisible({ timeout: 60_000 });
  await two.goto('./#/');
  await expect(two.getByRole('heading', { level: 2, name: /to clear today/ })).toBeVisible();
});
