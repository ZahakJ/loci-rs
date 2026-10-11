/**
 * Local HTTP transport for development and end-to-end tests: `loci serve --port 5741` exposes the
 * same RPC as the Tauri app.
 *
 *   POST /rpc        {method, args}                       → JSON result | {error}
 *   POST /rpc-bytes  {method, args}                       → raw bytes (404 when null)
 *   POST /rpc-with-bytes?method=…&args=…  raw body        → JSON result (BYTES_IN methods)
 *
 * Progress is not streamed over HTTP; long calls simply resolve when done.
 * The base URL comes from `VITE_LOCI_BRIDGE_URL`, else the dev default http://127.0.0.1:5741.
 */
import type { Progress } from '@core/types';
import type { Transport } from './index';
import type { RpcMethod } from './protocol';

/** A per-browser-context session id (sessionStorage), so `loci serve --multi-session` gives each Playwright context its own collection. */
function sessionId(): string {
  try {
    let id = sessionStorage.getItem('loci.session');
    if (!id) { id = Math.random().toString(36).slice(2) + Date.now().toString(36); sessionStorage.setItem('loci.session', id); }
    return id;
  } catch { return 'default'; }
}

export function httpBridgeUrl(): string {
  const env = (import.meta as unknown as { env?: Record<string, string | undefined> }).env;
  return env?.VITE_LOCI_BRIDGE_URL || 'http://127.0.0.1:5741';
}

async function fail(res: Response): Promise<never> {
  let body: unknown = null;
  try { body = await res.json(); } catch { /* not json */ }
  const err = (body && typeof body === 'object' && 'error' in body ? (body as { error: unknown }).error : body) as { code?: string; message?: string } | null;
  throw { code: err?.code ?? 'io', message: err?.message ?? `The engine answered ${res.status}` };
}

function downloadBlob(bytes: Uint8Array, filename: string, mime: string): void {
  const blob = new Blob([bytes as BlobPart], { type: mime });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 2000);
}

export function createHttpTransport(base: string): Transport {
  const url = (p: string) => `${base.replace(/\/$/, '')}${p}`;
  const session = sessionId();
  return {
    kind: 'http',
    async call(method: RpcMethod, args: unknown[], bytes: Uint8Array | null, _onProgress?: (p: Progress) => void): Promise<unknown> {
      let res: Response;
      if (bytes) {
        const q = new URLSearchParams({ method, args: JSON.stringify(args) });
        res = await fetch(url(`/rpc-with-bytes?${q}`), { method: 'POST', body: bytes as BodyInit, headers: { 'content-type': 'application/octet-stream', 'x-loci-session': session } });
      } else {
        res = await fetch(url('/rpc'), { method: 'POST', body: JSON.stringify({ method, args }), headers: { 'content-type': 'application/json', 'x-loci-session': session } });
      }
      if (!res.ok) await fail(res);
      const text = await res.text();
      return text === '' ? null : (JSON.parse(text) as unknown);
    },
    async callBytes(method: RpcMethod, args: unknown[]): Promise<Uint8Array | null> {
      const res = await fetch(url('/rpc-bytes'), { method: 'POST', body: JSON.stringify({ method, args }), headers: { 'content-type': 'application/json', 'x-loci-session': session } });
      if (res.status === 404) return null;
      if (!res.ok) await fail(res);
      const buf = new Uint8Array(await res.arrayBuffer());
      return buf.byteLength ? buf : null;
    },
    async saveFile(bytes: Uint8Array, filename: string, mime: string): Promise<string | null> {
      downloadBlob(bytes, filename, mime);
      return filename;
    },
  };
}
