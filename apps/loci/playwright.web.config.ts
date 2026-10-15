import { defineConfig } from '@playwright/test';

/**
 * End-to-end check of the browser build (the PWA with the WebAssembly engine) served from
 * `dist-web` under /loci-rs/app/, as on GitHub Pages. Run `pnpm build:wasm && pnpm build:web` first.
 * One worker, one headless system Chrome.
 */
export default defineConfig({
  testDir: 'tests/web',
  timeout: 120_000,
  expect: { timeout: 20_000 },
  workers: 1,
  retries: 0,
  reporter: [['list']],
  use: {
    channel: 'chrome',
    headless: true,
    baseURL: 'http://127.0.0.1:5767/loci-rs/app/',
    colorScheme: 'light',
    viewport: { width: 1280, height: 860 },
  },
  webServer: {
    command: 'VITE_LOCI_TARGET=web pnpm exec vite preview --port 5767 --strictPort --host 127.0.0.1',
    url: 'http://127.0.0.1:5767/loci-rs/app/',
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
