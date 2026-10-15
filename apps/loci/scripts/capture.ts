/**
 * README captures (internals §17): a REAL collection (the palace-legacy2 fixture imported through the
 * UI into the Rust engine), never the mock. Each screen at desktop 1440×900 (deviceScaleFactor 1)
 * and phone 390×844 (deviceScaleFactor 2), light and dark, into docs/captures/<screen>-<desktop|phone>-<light|dark>.png.
 *
 *   pnpm build && cargo build -p loci-cli && node --experimental-strip-types scripts/capture.ts
 *
 * Starts `loci serve --multi-session` (port 5745) and `vite preview` (port 5746) and stops them when
 * done. One headless Chrome (the system one).
 */
import { chromium, type Browser, type Page } from '@playwright/test';
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdir, readdir, stat } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const APP = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const REPO = path.dirname(path.dirname(APP));
const OUT = path.join(REPO, 'docs', 'captures');
const FIXTURE = path.join(REPO, 'fixtures', 'palace-legacy2.apkg');
const ENGINE_PORT = 5741;
const UI_PORT = 5746;
const BASE = `http://127.0.0.1:${UI_PORT}`;

type Theme = 'light' | 'dark';
interface Device { name: 'desktop' | 'phone'; width: number; height: number; scale: number; mobile: boolean }
const DEVICES: Device[] = [
  { name: 'desktop', width: 1440, height: 900, scale: 1, mobile: false },
  { name: 'phone', width: 390, height: 844, scale: 2, mobile: true },
];
const THEMES: Theme[] = ['light', 'dark'];

const log = (...a: unknown[]) => console.log('[capture]', ...a);
let step = 'start';
const at = (s: string) => { step = s; };

async function waitFor(url: string): Promise<void> {
  for (let i = 0; i < 100; i++) {
    try { if ((await fetch(url)).ok) return; } catch { /* not yet */ }
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error(`${url} did not come up`);
}

async function setTheme(page: Page, theme: Theme): Promise<void> {
  await page.emulateMedia({ colorScheme: theme });
  await page.evaluate((th) => { document.documentElement.dataset.theme = th; }, theme);
  await page.waitForTimeout(350);
}

async function clearToasts(page: Page): Promise<void> {
  const region = page.getByRole('region', { name: 'Notifications' });
  for (let i = 0; i < 6; i++) {
    const btn = region.getByRole('button', { name: 'Dismiss' });
    if (!(await btn.count())) break;
    await btn.first().click({ timeout: 2000 }).catch(() => undefined);
    await page.waitForTimeout(250);
  }
}

async function settle(page: Page): Promise<void> {
  await page.evaluate(() => document.fonts.ready.then(() => undefined));
  await page.waitForTimeout(500);
}

async function shoot(page: Page, device: Device, screen: string): Promise<void> {
  at(`${screen}: toasts`);
  await clearToasts(page);
  await page.mouse.move(device.width - 4, Math.round(device.height / 2));
  for (const theme of THEMES) {
    at(`${screen}: ${theme}`);
    await setTheme(page, theme);
    await settle(page);
    const file = path.join(OUT, `${screen}-${device.name}-${theme}.png`);
    await page.screenshot({ path: file, animations: 'disabled', caret: 'hide' });
    log(path.relative(REPO, file));
  }
  await setTheme(page, 'light');
}

async function importFixture(page: Page): Promise<void> {
  await page.goto(`${BASE}/#/import`);
  await page.locator('input[type=file]').setInputFiles(FIXTURE);
  await page.getByRole('button', { name: 'Import 402 new notes' }).click({ timeout: 30_000 });
  await page.getByRole('heading', { name: '402 notes imported' }).waitFor({ timeout: 60_000 });
}

async function arabicDeckHref(page: Page): Promise<string> {
  await page.goto(`${BASE}/#/`);
  const link = page.getByRole('list', { name: 'Decks' }).getByRole('link', { name: /^Arabic: / });
  await link.waitFor();
  return (await link.getAttribute('href'))!;
}

async function captureDevice(browser: Browser, device: Device): Promise<void> {
  const ctx = await browser.newContext({
    viewport: { width: device.width, height: device.height },
    deviceScaleFactor: device.scale,
    isMobile: device.mobile,
    hasTouch: device.mobile,
    colorScheme: 'light',
  });
  const page = await ctx.newPage();
  page.on('pageerror', (e) => log(`pageerror during ${device.name}/${step}:`, e.message.slice(0, 120)));
  page.on('console', (m) => { if (m.type() === 'error') log(`console error during ${device.name}/${step}:`, m.text().slice(0, 160)); });
  try {
    at('first run');
    await page.goto(`${BASE}/#/`);
    await page.getByRole('heading', { level: 1, name: /memory palace/ }).waitFor({ timeout: 20_000 });
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await shoot(page, device, 'first-run');

    await importFixture(page);

    await page.goto(`${BASE}/#/`);
    await page.getByRole('heading', { level: 2, name: /to clear today/ }).waitFor();
    await page.getByRole('region', { name: 'Momentum' }).waitFor();
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await shoot(page, device, 'home');

    if (!device.mobile) {
      at('palette');
      await page.keyboard.press('Control+k');
      const palette = page.getByRole('dialog', { name: 'Command palette' });
      await palette.waitFor();
      await palette.getByRole('combobox').fill('theme');
      await page.waitForTimeout(300);
      await shoot(page, device, 'palette');
      await page.keyboard.press('Escape');
      await palette.waitFor({ state: 'detached' });
    }

    at('study');
    await page.goto(`${BASE}/${await arabicDeckHref(page)}`);
    await page.getByRole('article', { name: 'Question' }).waitFor({ timeout: 20_000 });
    await page.waitForTimeout(600);
    await shoot(page, device, 'study-front');
    await page.keyboard.press('Space');
    await page.getByRole('article', { name: 'Answer' }).waitFor();
    const grades = page.getByRole('group', { name: 'How well did you remember?' });
    for (let i = 0; i < 12 && !(await grades.isVisible()); i++) { await page.keyboard.press('Space'); await page.waitForTimeout(150); }
    await page.waitForTimeout(600);
    await shoot(page, device, 'study-back');
    at('study: leave');
    await page.getByRole('button', { name: 'Leave session' }).click();

    at('add');
    await page.goto(`${BASE}/#/`);
    await page.getByRole('heading', { level: 2, name: /to clear today/ }).waitFor();
    await page.goto(`${BASE}/#/add`);
    const sheet = page.getByRole('dialog', { name: 'Add note' });
    await sheet.waitFor();
    await sheet.getByRole('button', { name: /^Note type / }).click();
    await page.getByRole('option', { name: 'Cloze', exact: true }).click();
    const text = sheet.getByRole('textbox', { name: 'Text' });
    await text.click();
    await page.keyboard.type('The method of loci places each item in a room of a familiar building.');
    await text.evaluate((el) => {
      const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
      for (let n = walker.nextNode(); n; n = walker.nextNode()) {
        const i = n.textContent!.indexOf('method of loci');
        if (i < 0) continue;
        const r = document.createRange();
        r.setStart(n, i);
        r.setEnd(n, i + 'method of loci'.length);
        window.getSelection()!.removeAllRanges();
        window.getSelection()!.addRange(r);
        return;
      }
    });
    await page.keyboard.press('Control+Shift+C');
    await sheet.getByRole('region', { name: 'Preview' }).getByText('[...]').first().waitFor();
    await page.evaluate(() => window.getSelection()?.removeAllRanges());
    await shoot(page, device, 'add-cloze');
    await page.keyboard.press('Escape');
    const discard = page.getByRole('button', { name: /^Discard/ });
    if (await discard.isVisible().catch(() => false)) await discard.click();

    at('browse');
    await page.goto(`${BASE}/#/browse?q=${encodeURIComponent('deck:Languages is:review')}`);
    const grid = page.getByRole('grid', { name: 'Search results' });
    await grid.getByRole('row').nth(3).waitFor();
    if (device.mobile) {
      await page.getByRole('button', { name: 'Select', exact: true }).click();
      await grid.getByRole('row').nth(1).click();
      await grid.getByRole('row').nth(3).click();
    } else {
      await grid.getByRole('row').nth(1).click();
      await grid.getByRole('row').nth(3).click({ modifiers: ['Shift'] });
    }
    await page.waitForTimeout(400);
    await shoot(page, device, 'browse');

    at('stats');
    await page.goto(`${BASE}/#/stats/all`);
    await page.getByRole('heading', { level: 2, name: 'Forecast' }).waitFor();
    await page.waitForTimeout(800);
    await shoot(page, device, 'stats');

    at('catch-up');
    await page.goto(`${BASE}/#/backlog/all`);
    await page.getByRole('heading', { level: 2, name: 'Pick a pace' }).waitFor();
    await shoot(page, device, 'backlog');

    at('settings');
    await page.goto(`${BASE}/#/settings`);
    await page.getByRole('heading', { level: 2, name: 'Appearance' }).waitFor();
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await shoot(page, device, 'settings');
  } finally {
    await ctx.close();
  }
}

await mkdir(OUT, { recursive: true });
const procs: ChildProcess[] = [];
const engine = spawn(path.join(REPO, 'target', 'debug', 'loci'), ['serve', '--multi-session', '--port', String(ENGINE_PORT)], { cwd: REPO, stdio: 'ignore' });
procs.push(engine);
const ui = spawn('pnpm', ['exec', 'vite', 'preview', '--port', String(UI_PORT), '--strictPort', '--host', '127.0.0.1'], {
  cwd: APP, stdio: 'ignore',
});
procs.push(ui);
let browser: Browser | null = null;
try {
  await waitFor(`http://127.0.0.1:${ENGINE_PORT}/health`);
  await waitFor(BASE);
  browser = await chromium.launch({ channel: 'chrome', headless: true });
  for (const d of DEVICES) await captureDevice(browser, d);
  let total = 0;
  for (const f of (await readdir(OUT)).filter((n) => n.endsWith('.png')).sort()) {
    const { size } = await stat(path.join(OUT, f));
    total += size;
  }
  log(`total ${(total / 1024 / 1024).toFixed(2)} MB in ${path.relative(REPO, OUT)}`);
} finally {
  await browser?.close();
  for (const p of procs) p.kill();
}
