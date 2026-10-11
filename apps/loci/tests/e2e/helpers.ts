import { expect, type Page } from '@playwright/test';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
export const FIXTURE = path.join(ROOT, '..', '..', 'fixtures', 'palace-legacy2.apkg');

/** Fails the test on uncaught page errors; returns the collected console errors for inspection. */
export function watchErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(`pageerror: ${e.message}`));
  page.on('console', (m) => { if (m.type() === 'error') errors.push(`console: ${m.text()}`); });
  return errors;
}

/** First run lands on Welcome. */
export async function openWelcome(page: Page): Promise<void> {
  await page.goto('/');
  await expect(page).toHaveURL(/#\/welcome$/);
  await expect(page.getByRole('heading', { level: 1, name: /memory palace/i })).toBeVisible();
}

/** Picks the fixture in the visible drop zone and waits for the preview. */
export async function pickFixture(page: Page, file = FIXTURE): Promise<void> {
  await page.locator('input[type=file]').setInputFiles(file);
  await expect(page).toHaveURL(/#\/import$/);
  await expect(page.getByRole('button', { name: /^Import \d[\d,]* (new )?notes$/ })).toBeVisible({ timeout: 30_000 });
}

/** Imports the fixture through #/import and lands on Home. */
export async function importFixture(page: Page): Promise<void> {
  await page.goto('/#/import');
  await expect(page.getByRole('heading', { level: 1, name: 'Import' })).toBeVisible();
  await pickFixture(page);
  await page.getByRole('button', { name: 'Import 402 new notes' }).click();
  await expect(page.getByRole('heading', { name: '402 notes imported' })).toBeVisible({ timeout: 60_000 });
  await page.getByRole('link', { name: 'Go to Home' }).click();
  await expect(page.getByRole('heading', { level: 2, name: /to clear today/ })).toBeVisible();
}

/** The study screen's counts group label, e.g. "18 new, 8 learning, 33 due". */
export function studyCounts(page: Page) {
  return page.getByRole('group', { name: /^\d+ new, \d+ learning, \d+ due$/ });
}

export async function countsLabel(page: Page): Promise<string> {
  return (await studyCounts(page).getAttribute('aria-label')) ?? '';
}

/** Waits for a study card's question side (Show answer button) to be ready. */
export async function waitForQuestion(page: Page): Promise<void> {
  await expect(page.getByRole('article', { name: 'Question' })).toBeVisible({ timeout: 20_000 });
  await expect(page.getByRole('button', { name: /^Show answer/ })).toBeEnabled();
}

/** Reveals with Space (and any one-by-one clozes) and grades with a number key; resolves when the answer side is gone. */
export async function gradeWithKeyboard(page: Page, key: '1' | '2' | '3' | '4' = '3'): Promise<void> {
  await waitForQuestion(page);
  await page.keyboard.press('Space');
  await expect(page.getByRole('article', { name: 'Answer' })).toBeVisible();
  const grades = page.getByRole('group', { name: 'How well did you remember?' });
  for (let i = 0; i < 12 && !(await grades.isVisible()); i++) {
    // "one by one" cloze answers reveal with Space before the grades appear
    await page.keyboard.press('Space');
    await page.waitForTimeout(150);
  }
  await expect(grades.getByRole('button', { name: /^Good/ })).toBeEnabled();
  await page.keyboard.press(key);
  await expect(page.getByRole('article', { name: 'Answer' })).toHaveCount(0, { timeout: 20_000 });
}

/** Horizontal overflow of the page in CSS px (0 = none). */
export async function horizontalOverflow(page: Page): Promise<number> {
  return page.evaluate(() => {
    const el = document.scrollingElement ?? document.documentElement;
    return el.scrollWidth - window.innerWidth;
  });
}
