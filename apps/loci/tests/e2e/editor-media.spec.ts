import { expect, test, type Page } from '@playwright/test';
import { importFixture, watchErrors } from './helpers';

// 2×2 red PNG
const PNG = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAFklEQVR4nGP4z8DAwMDAxMDAwMDAAAANHQEDasKb6QAAAABJRU5ErkJggg==',
  'base64',
);

function watch404(page: Page): string[] {
  const bad: string[] = [];
  page.on('response', (r) => { if (r.status() >= 400) bad.push(`${r.status()} ${r.url()}`); });
  page.on('requestfailed', (r) => { if (!r.url().startsWith('blob:')) bad.push(`failed ${r.url()}`); });
  return bad;
}

test('inserting an image into a field requests no bare file name and saves <img src="name">', async ({ page }) => {
  const errors = watchErrors(page);
  await importFixture(page);
  const bad = watch404(page);
  await page.getByRole('button', { name: 'Add A' }).click();
  const sheet = page.getByRole('dialog', { name: 'Add note' });
  await expect(sheet).toBeVisible();
  await sheet.getByRole('button', { name: /^Note type / }).click();
  await page.getByRole('option', { name: 'Basic', exact: true }).click();

  const front = sheet.getByRole('textbox', { name: 'Front' });
  await front.click();
  await page.keyboard.type('e2e image note ');
  const chooser = page.waitForEvent('filechooser');
  await sheet.getByRole('button', { name: 'Image' }).click();
  await (await chooser).setFiles({ name: 'red dot.png', mimeType: 'image/png', buffer: PNG });
  const img = front.locator('img');
  await expect(img).toHaveAttribute('src', /^blob:/);
  await expect(img).toHaveAttribute('data-loci-src', /\.png$/);
  const name = (await img.getAttribute('data-loci-src'))!;

  await sheet.getByRole('textbox', { name: 'Back' }).click();
  await page.keyboard.type('a red dot');
  await sheet.getByRole('button', { name: 'Add', exact: true }).click();
  await expect(page.getByRole('region', { name: 'Notifications' }).getByText(/^Added\./)).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(sheet).toHaveCount(0);

  // the saved note, loaded into Browse's editor (stored HTML → editor DOM)
  await page.goto('/#/browse');
  const box = page.getByRole('searchbox', { name: 'Search cards' });
  await box.fill('"e2e image note"');
  await box.press('Enter');
  const grid = page.getByRole('grid', { name: 'Search results' });
  await expect(page.getByRole('main').getByText(/^1 card$/).first()).toBeVisible();
  await grid.getByRole('row').nth(1).click();
  const editFront = page.getByRole('textbox', { name: 'Front' });
  await expect(editFront.locator('img')).toHaveAttribute('src', /^blob:/);
  await editFront.click();
  await page.keyboard.press('Control+Shift+X');
  const source = page.locator('textarea').first();
  await expect(source).toBeVisible();
  const html = await source.inputValue();
  expect(html).toContain(`<img src="${name}">`);
  expect(html).not.toContain('data-loci');
  expect(html).not.toContain('blob:');

  expect(bad).toEqual([]);
  expect(errors).toEqual([]);
});
