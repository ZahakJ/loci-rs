/**
 * Media URLs for rendered cards. The renderer references collection media as
 * `loci-media:<encodeURIComponent(name)>`; the UI swaps those for object URLs before the HTML
 * is sanitised (the sanitiser only lets http(s)/blob/data/relative URLs through).
 *
 * If the API exposes `getMediaUrl(name)` (the worker client's own cache) it is used; otherwise
 * bytes from `api.getMedia(name)` become a Blob URL cached here. Shared with the editor.
 */
import { ready } from '@ui/api';

export const MEDIA_PREFIX = 'loci-media:';
const MEDIA_RE = /loci-media:([^"'\s)<>]+)/g;

const cache = new Map<string, Promise<string>>();
const owned = new Set<string>();

const MIME: Record<string, string> = {
  png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', gif: 'image/gif', webp: 'image/webp', avif: 'image/avif',
  svg: 'image/svg+xml', bmp: 'image/bmp', ico: 'image/x-icon', tif: 'image/tiff', tiff: 'image/tiff',
  mp3: 'audio/mpeg', ogg: 'audio/ogg', oga: 'audio/ogg', opus: 'audio/ogg', wav: 'audio/wav', m4a: 'audio/mp4',
  aac: 'audio/aac', flac: 'audio/flac', spx: 'audio/ogg', webm: 'video/webm', mp4: 'video/mp4', m4v: 'video/mp4',
  mov: 'video/quicktime', ogv: 'video/ogg', mkv: 'video/x-matroska',
  woff2: 'font/woff2', woff: 'font/woff', ttf: 'font/ttf', otf: 'font/otf', css: 'text/css',
};

/** MIME type from a filename extension ('' when unknown). */
export function mimeForName(name: string): string {
  const dot = name.lastIndexOf('.');
  return dot >= 0 ? (MIME[name.slice(dot + 1).toLowerCase()] ?? '') : '';
}

function decodeName(encoded: string): string {
  try { return decodeURIComponent(encoded); } catch { return encoded; }
}

type MaybeUrlApi = { getMediaUrl?: (name: string) => Promise<string | null> | string | null };

async function load(name: string): Promise<string> {
  const a = await ready;
  const direct = (a as unknown as MaybeUrlApi).getMediaUrl;
  if (typeof direct === 'function') {
    try {
      const url = await direct.call(a, name);
      if (url) return url;
    } catch { /* fall back to bytes */ }
  }
  const bytes = await a.getMedia(name);
  if (!bytes) return '';
  const url = URL.createObjectURL(new Blob([bytes as BlobPart], { type: mimeForName(name) }));
  owned.add(url);
  return url;
}

/** Object URL for a collection media file ('' when missing). Cached per name. */
export function resolveMediaUrl(name: string): Promise<string> {
  let p = cache.get(name);
  if (!p) {
    p = load(name).catch(() => '');
    cache.set(name, p);
    // Missing files are retried next time (the user may add them).
    void p.then((u) => { if (!u) cache.delete(name); });
  }
  return p;
}

/** Names referenced as `loci-media:` in a string. */
export function mediaNamesIn(text: string): string[] {
  const out = new Set<string>();
  for (const m of text.matchAll(MEDIA_RE)) out.add(decodeName(m[1]!));
  return [...out];
}

/** Replaces every `loci-media:` reference with its object URL (missing files become ''). */
export async function resolveMediaIn(text: string): Promise<string> {
  if (!text.includes(MEDIA_PREFIX)) return text;
  const names = mediaNamesIn(text);
  const urls = new Map<string, string>();
  await Promise.all(names.map(async (n) => urls.set(n, await resolveMediaUrl(n))));
  return text.replace(MEDIA_RE, (_m, enc: string) => urls.get(decodeName(enc)) ?? '');
}

/** URL for a media reference that may already be a URL (data:, blob:, http) or a bare/prefixed name. */
export async function resolveMediaRef(ref: string): Promise<string> {
  if (!ref) return '';
  if (ref.startsWith(MEDIA_PREFIX)) return resolveMediaUrl(decodeName(ref.slice(MEDIA_PREFIX.length)));
  if (/^(?:data|blob|https?):/i.test(ref)) return ref;
  return resolveMediaUrl(ref);
}

/** Drops cached URLs (e.g. after media changed). Object URLs we created are revoked. */
export function clearMediaCache(names?: string[]): void {
  const list = names ?? [...cache.keys()];
  for (const n of list) {
    const p = cache.get(n);
    cache.delete(n);
    void p?.then((u) => { if (owned.has(u)) { URL.revokeObjectURL(u); owned.delete(u); } });
  }
}
