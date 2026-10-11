import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { fileURLToPath } from 'node:url';

const r = (p: string) => fileURLToPath(new URL(p, import.meta.url));
const host = process.env.TAURI_DEV_HOST;

// Tauri dev server: fixed port, no screen clearing, mobile host when the Android dev loop sets it.
export default defineConfig({
  plugins: [react()],
  resolve: { alias: { '@core': r('./src/core'), '@ui': r('./src/ui') } },
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
    target: process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari13',
    // CSS must keep logical properties as they are: lowered for safari13, `inset-inline-end` became
    // `:not(:lang(ar, he, …))` rules that Chromium (Android webview, Windows, the e2e browser) drops,
    // which put every field suffix and select chevron over the start of the value (audit 0.1 §2).
    cssTarget: ['chrome105', 'safari15'],
    minify: !process.env.TAURI_ENV_DEBUG,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
});
