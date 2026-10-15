import { expect, test } from '@playwright/test';
import { countsLabel, gradeWithKeyboard, pickFixture, waitForQuestion, watchErrors } from '../e2e/helpers';

test('the browser build imports, studies, keeps its data across reloads and works offline', async ({ page, context }) => {
  const errors = watchErrors(page);
  await page.goto('./');
  await expect(page).toHaveURL(/#\/welcome$/);
  expect(await page.evaluate(() => navigator.serviceWorker.ready.then((r) => !!r.active))).toBe(true);

  await page.goto('./#/import');
  await expect(page.getByRole('heading', { level: 1, name: 'Import' })).toBeVisible();
  await pickFixture(page);
  await page.getByRole('button', { name: 'Import 402 new notes' }).click();
  await expect(page.getByRole('heading', { name: '402 notes imported' })).toBeVisible({ timeout: 60_000 });
  await page.getByRole('link', { name: 'Go to Home' }).click();
  await expect(page.getByRole('heading', { level: 2, name: /to clear today/ })).toBeVisible();

  await page.getByRole('button', { name: /^Start/ }).click();
  await waitForQuestion(page);
  await gradeWithKeyboard(page, '3');
  await waitForQuestion(page);
  const afterAnswer = await countsLabel(page);

  // the collection lives in OPFS: a reload finds the import and the answer
  await page.reload();
  await page.goto('./#/');
  await expect(page.getByRole('heading', { level: 2, name: /to clear today/ })).toBeVisible();
  await page.getByRole('button', { name: /^Start/ }).click();
  await waitForQuestion(page);
  expect(await countsLabel(page)).toBe(afterAnswer);

  // a second tab is refused while this one owns the collection
  const other = await context.newPage();
  await other.goto('./');
  await expect(other.getByRole('heading', { name: 'Loci is open in another tab' })).toBeVisible();
  await other.close();

  // offline: the service worker serves the app, OPFS the data
  await context.setOffline(true);
  await page.reload();
  await page.goto('./#/');
  await expect(page.getByRole('heading', { level: 2, name: /to clear today/ })).toBeVisible().catch(async (e: unknown) => {
    console.log('OFFLINE BODY:', await page.locator('body').innerText().catch(() => '?'), '\nERRORS:', errors.join('\n'));
    throw e;
  });
  await context.setOffline(false);

  expect(errors.filter((e) => !/Failed to load resource/.test(e))).toEqual([]);
});
