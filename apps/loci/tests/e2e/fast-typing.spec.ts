import { expect, test } from '@playwright/test';
import { importFixture } from './helpers';

const TEXT = 'The method of loci places each item along a familiar route so that walking it again recalls them in order; '
  + 'Simonides is said to have found it when he named the guests of a collapsed hall by where they had sat at the table. ';

async function typeFast(page: import('@playwright/test').Page) {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => { if (m.type() === 'error') errors.push(`console: ${m.text()}`); });
  await importFixture(page);
  await page.getByRole('button', { name: 'Add A' }).click();
  const sheet = page.getByRole('dialog', { name: 'Add note' });
  await expect(sheet).toBeVisible();
  const text = TEXT.slice(0, 200);
  for (const [type, fields] of [['Basic', ['Front', 'Back']], ['Cloze', ['Text', 'Back Extra']]] as const) {
    await sheet.getByRole('button', { name: /^Note type / }).click();
    await page.getByRole('option', { name: type, exact: true }).click();
    await expect(sheet.getByRole('button', { name: `Note type ${type}` })).toBeVisible();
    for (const name of fields) {
      const box = sheet.getByRole('textbox', { name, exact: true });
      await box.click();
      await page.keyboard.type(text, { delay: 0 });
      await expect(box).toContainText(text.slice(-20).trim());
      const lines = await box.evaluate((el) => Math.round(el.getBoundingClientRect().height / parseFloat(getComputedStyle(el).lineHeight || '20')));
      expect(lines, `${type} ${name} wraps`).toBeGreaterThan(1);
    }
    await page.waitForTimeout(500);
  }
  expect(errors).toEqual([]);
}

test('typing fast into Add-sheet fields until they wrap throws no page error (Basic and Cloze)', async ({ page }) => { await typeFast(page); });

test.describe('phone', () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true });
  test('typing fast on a phone-sized Add screen throws no page error', async ({ page }) => { await typeFast(page); });
});
