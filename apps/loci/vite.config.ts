import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { fileURLToPath } from 'node:url';
import { pwa } from './scripts/pwa';

const r = (p: string) => fileURLToPath(new URL(p, import.meta.url));
const host = process.env.TAURI_DEV_HOST;
// The browser build (WebAssembly engine, PWA) served under GitHub Pages at /loci-rs/app/.
const web = process.env.VITE_LOCI_TARGET === 'web';

// Tauri dev server: fixed port, no screen clearing, mobile host when the Android dev loop sets it.
export default defineConfig({
  plugins: web ? [react(), pwa()] : [react()],
  base: web ? '/loci-rs/app/' : '/',
  resolve: {
    alias: {
      '@core': r('./src/core'),
      '@ui': r('./src/ui'),
      // the engine worker and its wasm-bindgen module (crates/loci-web, scripts/build-wasm.sh) exist
      // only in the browser build; other builds get a stub and ship no worker
      '@loci-web-spawn': r(web ? './src/bridge/wasm-spawn.ts' : './src/bridge/wasm-spawn-stub.ts'),
      '@loci-web': r('./wasm/loci_web.js'),
    },
  },
  worker: { format: 'es' },
  clearScreen: false,
  server: {
    port: 5740,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 5743 } : undefined,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: {
    outDir: web ? 'dist-web' : 'dist',
    // the browser build needs module workers and WebAssembly; safari15 covers both
    target: web ? 'safari15' : process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari13',
    // CSS must keep logical properties as they are: lowered for safari13, `inset-inline-end` became
    // `:not(:lang(ar, he, …))` rules that Chromium (Android webview, Windows, the e2e browser) drops,
    // which put every field suffix and select chevron over the start of the value (audit 0.1 §2).
    cssTarget: ['chrome105', 'safari15'],
    minify: !process.env.TAURI_ENV_DEBUG,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
});
