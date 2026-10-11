import { expect, test, type Page } from '@playwright/test';
import { importFixture, watchErrors } from './helpers';

test.use({ colorScheme: 'dark' });

/** Every element in each card shadow root: no black text, no white/light panels; the content root carries nightMode. */
async function cardColours(page: Page) {
  return page.evaluate(() => {
    const lum = (c: string): { l: number; a: number } | null => {
      const m = c.match(/-?[\d.]+/g);
      if (!m || m.length < 3) return null;
      const n = m.map(Number);
      const alpha = c.includes('/') || c.startsWith('rgba') ? n[n.length - 1]! : 1;
      if (/^(?:ok)?lab\(|^(?:ok)?lch\(/.test(c)) {
        const L = n[0]!;
        return { l: c.startsWith('ok') ? (L > 1 ? L / 100 : L) : L / 100, a: alpha };
      }
      if (c.startsWith('rgb')) return { l: (0.2126 * n[0]! + 0.7152 * n[1]! + 0.0722 * n[2]!) / 255, a: alpha };
      return null;
    };
    const out: { roots: number; nightMode: boolean[]; dark: string[]; light: string[] } = { roots: 0, nightMode: [], dark: [], light: [] };
    for (const host of Array.from(document.querySelectorAll('*'))) {
      const root = host.shadowRoot;
      if (!root) continue;
      const card = root.querySelector('.card');
      if (!card) continue;
      out.roots++;
      out.nightMode.push(card.classList.contains('nightMode'));
      for (const el of [card, ...Array.from(card.querySelectorAll('*'))]) {
        if ((el as HTMLElement).offsetParent === null && el !== card) continue;
        const cs = getComputedStyle(el);
        const fg = lum(cs.color);
        const text = (el.textContent ?? '').trim();
        if (fg && fg.a > 0 && fg.l < 0.35 && text && !['svg', 'path', 'rect'].includes(el.tagName.toLowerCase())) out.dark.push(`${el.tagName}.${el.className} color ${cs.color}`);
        const bg = lum(cs.backgroundColor);
        if (bg && bg.a > 0.5 && bg.l > 0.7) out.light.push(`${el.tagName}.${el.className} background ${cs.backgroundColor}`);
      }
    }
    return out;
  });
}

test('dark theme: imported Anki stock CSS paints no white panels or black text (study and editor preview)', async ({ page }) => {
  const errors = watchErrors(page);
  await importFixture(page);
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  for (const q of ['note:Basic', 'note:Cloze', '"note:Image Occlusion"', 'note:Vocab', '"note:Basic (type in the answer)"']) {
    await page.goto('/#/study/all?custom=' + encodeURIComponent(`search:${q}`));
    await expect(page.getByRole('article', { name: 'Question' })).toBeVisible({ timeout: 20_000 });
    await page.waitForTimeout(300);
    let c = await cardColours(page);
    expect(c.roots, q).toBeGreaterThan(0);
    expect(c.nightMode.every(Boolean), q).toBe(true);
    expect(c.dark, q).toEqual([]);
    expect(c.light, q).toEqual([]);
    const check = page.getByRole('button', { name: /^Check/ });
    if (await check.isVisible()) await check.click();
    else await page.getByRole('button', { name: /^Show answer/ }).click();
    await expect(page.getByRole('article', { name: 'Answer' })).toBeVisible();
    await page.waitForTimeout(300);
    c = await cardColours(page);
    expect(c.dark, `${q} answer`).toEqual([]);
    expect(c.light, `${q} answer`).toEqual([]);
    await page.goto('/#/');
  }

  // editor preview of a Cloze note
  await page.getByRole('button', { name: 'Add A' }).click();
  const sheet = page.getByRole('dialog', { name: 'Add note' });
  await sheet.getByRole('button', { name: /^Note type / }).click();
  await page.getByRole('option', { name: 'Cloze', exact: true }).click();
  await sheet.getByRole('textbox', { name: 'Text' }).click();
  await page.keyboard.type('Dark {{c1::preview}} text');
  // Back Extra carries the inline black-on-white styles pasted content brings from Anki
  await sheet.getByRole('textbox', { name: 'Back Extra' }).click();
  await page.keyboard.press('Control+Shift+X');
  await sheet.locator('textarea').first().fill('<span style="color: rgb(0, 0, 0); background-color: rgb(255, 255, 255);">pasted extra</span> <font color="#000000">old font</font>');
  const preview = sheet.getByRole('region', { name: 'Preview' });
  await expect(preview.getByText('[...]').first()).toBeVisible();
  // show the back so Back Extra renders
  await preview.getByRole('radio', { name: 'Back' }).click();
  await expect.poll(() => page.evaluate(() => Array.from(document.querySelectorAll('*')).some((h) => h.shadowRoot?.textContent?.includes('pasted extra')))).toBe(true);
  const c = await cardColours(page);
  expect(c.roots).toBeGreaterThan(0);
  expect(c.nightMode.every(Boolean)).toBe(true);
  expect(c.dark).toEqual([]);
  expect(c.light).toEqual([]);
  expect(errors).toEqual([]);
});
