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
    minify: !process.env.TAURI_ENV_DEBUG,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
});
