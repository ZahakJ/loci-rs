import { expect, test } from '@playwright/test';
import { openWelcome, pickFixture, watchErrors } from './helpers';

test.describe('first run', () => {
  test('(a) Welcome shows on first run; the sample deck installs with 20 new', async ({ page }) => {
    const errors = watchErrors(page);
    await openWelcome(page);
    await expect(page.getByRole('heading', { name: 'Try a sample deck' })).toBeVisible();
    await expect(page.getByText('Import from Anki')).toBeVisible();
    await page.getByRole('button', { name: 'Try the sample' }).click();
    await expect(page).toHaveURL(/#\/$/);
    const hero = page.getByRole('region', { name: /to clear today/ });
    await expect(hero.getByText('20 new cards')).toBeVisible();
    const deck = page.getByRole('list', { name: 'Decks' }).getByRole('link', { name: /^Loci: the art of memory: 20 new/ });
    await expect(deck).toBeVisible();
    // Start is focused on arrival (internals §16)
    await expect(page.getByRole('button', { name: /^Start/ })).toBeFocused();
    expect(errors.filter((e) => e.startsWith('pageerror'))).toEqual([]);
  });

  test('(b) importing the fixture from the Welcome drop zone keeps scheduling and survives a reload', async ({ page }) => {
    const errors = watchErrors(page);
    await openWelcome(page);
    await pickFixture(page);
    // the preview: 402 notes / 590 cards / 7,774 reviews
    const main = page.getByRole('main');
    for (const [n, label] of [['402', 'Notes'], ['590', 'Cards'], ['7,774', 'Reviews']] as const) {
      await expect(main.getByText(n, { exact: true })).toBeVisible();
      await expect(main.getByText(label, { exact: true })).toBeVisible();
    }
    await expect(main.getByText(/Scheduling found/)).toBeVisible();
    await page.getByRole('button', { name: 'Import 402 new notes' }).click();
    await expect(page.getByRole('heading', { name: '402 notes imported' })).toBeVisible({ timeout: 60_000 });
    await page.getByRole('link', { name: 'Go to Home' }).click();

    const checkHome = async () => {
      const hero = page.getByRole('region', { name: /to clear today/ });
      await expect(hero).toBeVisible();
      const due = Number((await hero.getByText(/\d+ reviews? due/).innerText()).match(/\d+/)![0]);
      expect(due).toBeGreaterThan(0);
      await expect(page.getByRole('complementary', { name: 'Catch-up' })).toBeVisible();
      await expect(page.getByRole('complementary', { name: 'Catch-up' }).getByRole('link', { name: 'Plan a catch-up' })).toBeVisible();
      const decks = page.getByRole('list', { name: 'Decks' });
      for (const name of ['History', 'Languages', 'Arabic', 'Medicine', 'Science']) {
        await expect(decks.getByRole('link', { name: new RegExp(`^${name}: `) })).toBeVisible();
      }
      return due;
    };
    const due = await checkHome();
    await page.reload();
    expect(await checkHome()).toBe(due);
    await expect(page.getByText('590 cards in all')).toBeVisible();
    expect(errors.filter((e) => e.startsWith('pageerror'))).toEqual([]);
  });
});
