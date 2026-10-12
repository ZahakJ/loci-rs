/**
 * Renders docs/media/banner.html to docs/media/banner.png (1640×640) with the system Chrome.
 * The banner shows real captures from docs/captures, so run `scripts/capture.ts` first.
 *
 *   node --experimental-strip-types scripts/banner.ts
 */
import { chromium } from '@playwright/test';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', '..');
const SRC = path.join(REPO, 'docs', 'media', 'banner.html');
const OUT = path.join(REPO, 'docs', 'media', 'banner.png');

const browser = await chromium.launch({ channel: 'chrome', headless: true, args: ['--allow-file-access-from-files'] });
try {
  const page = await browser.newPage({ viewport: { width: 1640, height: 640 }, deviceScaleFactor: 1 });
  await page.goto(pathToFileURL(SRC).href);
  await page.evaluate(() => document.fonts.ready.then(() => undefined));
  await page.waitForTimeout(300);
  await page.screenshot({ path: OUT });
  console.log(path.relative(REPO, OUT));
} finally {
  await browser.close();
}
