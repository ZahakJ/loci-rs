import { expect, test } from '@playwright/test';
import { importFixture, watchErrors } from './helpers';

/** The designed Select (ARIA select-only combobox) by mouse and by keyboard, inside a page and a phone viewport. */
test('Select: pick by click and by keyboard; the choice is saved; Esc closes only the list', async ({ page }) => {
  const errors = watchErrors(page);
  await importFixture(page);
  await page.goto('/#/settings?section=study');
  const day = page.getByRole('combobox', { name: 'New day starts at' });
  await expect(day).toHaveText('4 AM');
  await expect(page.locator('select')).toHaveCount(0);

  await day.click();
  const list = page.getByRole('listbox');
  await expect(list).toBeVisible();
  await expect(day).toHaveAttribute('aria-expanded', 'true');
  await expect(list.getByRole('option', { name: '4 AM' })).toHaveAttribute('aria-selected', 'true');
  await list.getByRole('option', { name: '6 AM' }).click();
  await expect(list).toBeHidden();
  await expect(day).toHaveText('6 AM');
  await expect(day).toBeFocused();

  // keyboard: open with ArrowDown, move, Enter picks
  await day.press('ArrowDown');
  await expect(list).toBeVisible();
  await expect(page.getByRole('option', { name: '6 AM' })).toBeFocused();
  await page.keyboard.press('ArrowUp');
  await page.keyboard.press('Enter');
  await expect(day).toHaveText('5 AM');

  // Escape closes the list and keeps the value
  await day.press('Enter');
  await page.keyboard.press('Home');
  await page.keyboard.press('Escape');
  await expect(list).toBeHidden();
  await expect(day).toHaveText('5 AM');

  await page.reload();
  await expect(page.getByRole('combobox', { name: 'New day starts at' })).toHaveText('5 AM');
  expect(errors).toEqual([]);
});

test('phone: the list stays above the tab bar', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await importFixture(page);
  await page.goto('/#/settings?section=study');
  const day = page.getByRole('combobox', { name: 'New day starts at' });
  await day.scrollIntoViewIfNeeded();
  await day.click();
  const list = page.getByRole('listbox');
  await expect(list).toBeVisible();
  const box = (await list.boundingBox())!;
  const bar = (await page.getByRole('navigation').last().boundingBox())!;
  expect(box.y + box.height).toBeLessThanOrEqual(bar.y);
});

test('Select inside a dialog: Esc closes the list, not the dialog; Tab leaves from the trigger', async ({ page }) => {
  await importFixture(page);
  await page.goto('/#/');
  await page.getByRole('button', { name: 'Actions for History' }).click();
  await page.getByRole('menuitem', { name: 'Options' }).click();
  const dialog = page.getByRole('dialog', { name: /^Options for/ });
  await expect(dialog).toBeVisible();
  const preset = dialog.getByRole('combobox');
  await expect(preset).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('listbox')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('listbox')).toBeHidden();
  await expect(dialog).toBeVisible();
  await expect(preset).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Tab');
  await expect(page.getByRole('listbox')).toBeHidden();
  await expect(dialog).toBeVisible();
  await expect(preset).not.toBeFocused();
  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();
});
