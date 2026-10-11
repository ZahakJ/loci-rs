import { expect, test, type Page } from '@playwright/test';
import { gradeWithKeyboard, importFixture, waitForQuestion } from './helpers';

/** The focused element: exists, is not <body>, and draws a visible focus indicator. */
async function focusInfo(page: Page) {
  return page.evaluate(() => {
    const el = document.activeElement as HTMLElement | null;
    if (!el || el === document.body || el === document.documentElement) return null;
    const cs = getComputedStyle(el);
    const outline = cs.outlineStyle !== 'none' && parseFloat(cs.outlineWidth) > 0;
    const ring = cs.boxShadow !== 'none' && cs.boxShadow !== '';
    // stretched links draw their ring on ::after (DeckTree)
    const after = getComputedStyle(el, '::after');
    const afterRing = after.outlineStyle !== 'none' && parseFloat(after.outlineWidth) > 0;
    const r = el.getBoundingClientRect();
    return { tag: el.tagName, name: el.getAttribute('aria-label') ?? el.textContent?.trim().slice(0, 40) ?? '', visible: outline || ring || afterRing, w: r.width, h: r.height, focusVisible: el.matches(':focus-visible') };
  });
}

test('(i) keyboard focus is visible on Home and Study; the study live region announces grades', async ({ page }) => {
  await importFixture(page);
  await page.goto('/#/');
  await expect(page.getByRole('button', { name: /^Start/ })).toBeVisible();
  const seen = new Set<string>();
  for (let i = 0; i < 12; i++) {
    await page.keyboard.press('Tab');
    const f = await focusInfo(page);
    expect(f, `Tab ${i + 1} left focus on <body>`).not.toBeNull();
    expect(f!.w * f!.h).toBeGreaterThan(0);
    expect(f!.visible, `no visible focus indicator on ${f!.tag} "${f!.name}"`).toBe(true);
    seen.add(`${f!.tag}:${f!.name}`);
  }
  expect(seen.size).toBeGreaterThan(5);

  await page.getByRole('button', { name: /^Start/ }).click();
  await waitForQuestion(page);
  await expect(page.getByRole('status').filter({ hasText: 'Question shown' })).toHaveCount(1);
  await gradeWithKeyboard(page, '3');
  await waitForQuestion(page);
  await expect(page.getByRole('status').filter({ hasText: /^Graded Good/ })).toHaveCount(1);

  // tab through the study screen's controls: forward from the card to the answer bar, then back to the top bar
  await page.keyboard.press('Tab');
  let f = await focusInfo(page);
  expect(f, 'Tab from the card left focus on <body>').not.toBeNull();
  expect(f!.name).toMatch(/^Show answer/);
  expect(f!.visible).toBe(true);
  const back: string[] = [];
  for (let i = 0; i < 2; i++) {
    await page.keyboard.press('Shift+Tab');
    f = await focusInfo(page);
    expect(f, `study Shift+Tab ${i + 1} left focus on <body>`).not.toBeNull();
    expect(f!.visible, `no visible focus indicator on ${f!.tag} "${f!.name}"`).toBe(true);
    back.push(f!.name);
  }
  expect(back).toEqual(['More actions', 'Leave session']);
});
