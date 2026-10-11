import { expect, test } from '@playwright/test';
import { horizontalOverflow, importFixture, waitForQuestion } from './helpers';

test.use({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true });

test('(j) phone 390x844: Home, Study and Browse have no horizontal scroll', async ({ page }) => {
  await importFixture(page);
  const width = async () => page.evaluate(() => (document.scrollingElement ?? document.documentElement).scrollWidth);

  await page.goto('/#/');
  await expect(page.getByRole('region', { name: /to clear today/ })).toBeVisible();
  await expect(page.getByRole('list', { name: 'Decks' })).toBeVisible();
  expect(await width(), 'Home').toBeLessThanOrEqual(390);

  await page.goto('/#/study/all');
  await waitForQuestion(page);
  expect(await width(), 'Study front').toBeLessThanOrEqual(390);
  await page.getByRole('button', { name: /^Show answer/ }).click();
  await expect(page.getByRole('group', { name: 'How well did you remember?' }).or(page.getByRole('button', { name: /^Reveal next/ }))).toBeVisible();
  expect(await width(), 'Study back').toBeLessThanOrEqual(390);

  await page.goto('/#/browse?q=deck%3ALanguages');
  await expect(page.getByRole('grid', { name: 'Search results' })).toBeVisible();
  expect(await width(), 'Browse').toBeLessThanOrEqual(390);
  expect(await horizontalOverflow(page)).toBeLessThanOrEqual(0);
});
