import { expect, test } from '@playwright/test';
import { countsLabel, importFixture, studyCounts, waitForQuestion, watchErrors } from './helpers';

test('extend today\'s limits (Custom session): after the day\'s new cards are used up, the next session serves new cards', async ({ page }) => {
  const errors = watchErrors(page);
  await importFixture(page);
  await page.getByRole('list', { name: 'Decks' }).getByRole('link', { name: /^Science: / }).click();
  await expect(page).toHaveURL(/#\/study\/\d+$/);
  await waitForQuestion(page);
  // learn the day's new cards (Easy graduates at once)
  const answerEasy = async () => {
    const check = page.getByRole('button', { name: /^Check/ });
    if (await check.isVisible()) await check.click();
    else await page.getByRole('button', { name: /^Show answer/ }).click();
    const grades = page.getByRole('group', { name: 'How well did you remember?' });
    for (let k = 0; k < 12 && !(await grades.isVisible()); k++) { await page.keyboard.press('Space'); await page.waitForTimeout(150); }
    await grades.getByRole('button', { name: /^Easy/ }).click();
    await expect(page.getByRole('article', { name: 'Answer' })).toHaveCount(0, { timeout: 20_000 });
  };
  for (let i = 0; i < 20; i++) {
    await page.waitForTimeout(300);
    if (!(await page.getByRole('article', { name: 'Question' }).isVisible())) break;
    await answerEasy();
  }
  // the day's new cards are used up: the summary (or empty state) offers more
  await page.getByRole('button', { name: /^(Study more|Custom session)$/ }).first().click();
  const sheet = page.getByRole('dialog', { name: 'Custom session' });
  await sheet.getByRole('radio', { name: /^Extend today's limits/ }).check();
  await sheet.getByRole('button', { name: 'Start session' }).click();
  await waitForQuestion(page);
  const label = await countsLabel(page);
  const fresh = Number(label.match(/^(\d+) new/)![1]);
  expect(fresh).toBeGreaterThan(0);
  expect(fresh).toBeLessThanOrEqual(10);
  expect(errors).toEqual([]);
});
