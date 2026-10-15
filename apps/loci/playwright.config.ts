import { defineConfig } from '@playwright/test';

/**
 * e2e against the production frontend build served by `vite preview`, talking to the real Rust
 * engine through `loci serve --multi-session` (internals §17). Run `pnpm build` and
 * `cargo build -p loci-cli` first. One worker, one headless system Chrome; every test gets a
 * fresh browser context, and therefore (through the x-loci-session header) a fresh collection.
 */
export default defineConfig({
  testDir: 'tests/e2e',
  timeout: 90_000,
  expect: { timeout: 15_000 },
  workers: 1,
  fullyParallel: false,
  retries: 0,
  reporter: [['list']],
  use: {
    channel: 'chrome',
    headless: true,
    baseURL: 'http://127.0.0.1:5742',
    colorScheme: 'light',
    viewport: { width: 1440, height: 900 },
    trace: 'retain-on-failure',
  },
  webServer: [
    {
      command: '../../target/debug/loci serve --multi-session --port 5741',
      url: 'http://127.0.0.1:5741/health',
      reuseExistingServer: true,
      timeout: 60_000,
    },
    {
      command: 'pnpm preview',
      url: 'http://127.0.0.1:5742',
      reuseExistingServer: true,
      timeout: 60_000,
    },
  ],
});
