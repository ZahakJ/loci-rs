import { expect, test } from '@playwright/test';
import { countsLabel, gradeWithKeyboard, importFixture, studyCounts, waitForQuestion } from './helpers';

test('(c) study five cards with the keyboard, see the history, undo with z', async ({ page }) => {
  await importFixture(page);
  await page.getByRole('button', { name: /^Start/ }).click();
  await expect(page).toHaveURL(/#\/study\/all$/);
  await waitForQuestion(page);
  await expect(studyCounts(page)).toBeVisible();

  const labels: string[] = [await countsLabel(page)];
  for (let i = 0; i < 5; i++) {
    await gradeWithKeyboard(page, '3');
    await waitForQuestion(page);
    labels.push(await countsLabel(page));
  }
  // the remaining counts moved after grading (new/review shrink, learning may grow)
  expect(labels[5]).not.toBe(labels[0]);
  const sum = (l: string) => (l.match(/\d+/g) ?? []).map(Number).reduce((a, b) => a + b, 0);
  expect(sum(labels[5]!)).toBeLessThan(sum(labels[0]!));

  // undo the fifth answer with z: the counts go back to what they were before it
  await page.keyboard.press('z');
  await expect(page.getByRole('region', { name: 'Notifications' }).getByText(/^Undone/)).toBeVisible();
  await waitForQuestion(page);
  await expect(studyCounts(page)).toHaveAttribute('aria-label', labels[4]!);

  // card info for the card the undo brought back
  await page.keyboard.press('i');
  const sheet = page.getByRole('dialog', { name: 'Why this interval?' });
  await expect(sheet).toBeVisible();
  await expect(sheet.getByRole('heading', { name: 'Review history' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(sheet).toHaveCount(0);

  // grade the restored card again so today's total is back to five
  await gradeWithKeyboard(page, '3');
  await waitForQuestion(page);

  // revlog-dependent: today's statistics count the reviews just made (5 answered, 1 undone, 1 more)
  await page.getByRole('button', { name: 'Leave session' }).click();
  await expect(page).toHaveURL(/#\/$/);
  await page.goto('/#/stats/all');
  const today = page.getByRole('region', { name: 'Today' });
  await expect(today.getByText(/^Studied 5 cards in/)).toBeVisible();
  await expect(page.getByRole('link', { name: 'Browse B' })).toBeVisible();
  // and Browse's "Studied today" filter finds them (rated:1)
  await page.goto('/#/browse?q=rated%3A1');
  await expect(page.getByRole('main').getByText(/^5 cards$/).first()).toBeVisible();
});

test('(c2) card info lists the review history of an imported card', async ({ page }) => {
  await importFixture(page);
  // a review card that is due today has reviews in the fixture's revlog
  await page.goto('/#/study/all?custom=' + encodeURIComponent('search:is:review is:due'));
  await waitForQuestion(page);
  await page.keyboard.press('i');
  const sheet = page.getByRole('dialog', { name: 'Why this interval?' });
  await expect(sheet.getByRole('heading', { name: 'Review history' })).toBeVisible();
  await expect(sheet.getByRole('listitem').first()).toBeVisible();
  expect(await sheet.getByRole('listitem').count()).toBeGreaterThan(0);
});
