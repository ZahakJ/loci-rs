/**
 * Media inside the field editor. Stored HTML references collection files by bare name
 * (`<img src="photo.jpg">`, as Anki does); in the live editor those become object URLs from the
 * shared card resolver (`@ui/card/media`), with the original name kept in `data-loci-src` so
 * saving writes the name back. `[sound:x]` stays text.
 */
import { api } from '@ui/api';
import { resolveMediaRef } from '@ui/card/media';
import { SEL_END, SEL_START } from './cloze';
import { pastedFileName } from './paste';

export const isUrlSrc = (src: string): boolean => /^(?:data|blob|https?):/i.test(src.trim());

const MEDIA_SEL = 'img, audio, video, source';

/**
 * Moves bare media names from `src` to `data-loci-src` under `root`, leaving no `src`, so the
 * browser never requests the bare name (it would 404: files live in the collection, not on the
 * server). Safe on inert trees (a <template>'s content), where nothing is fetched.
 */
export function deferMediaSrc(root: ParentNode): void {
  for (const el of Array.from(root.querySelectorAll<HTMLElement>(MEDIA_SEL))) {
    const src = el.getAttribute('src');
    if (!src || isUrlSrc(src)) continue;
    if (!el.dataset.lociSrc) el.dataset.lociSrc = src;
    el.removeAttribute('src');
  }
}

/** `html` with bare media names deferred (see deferMediaSrc), for execCommand('insertHTML'). */
export function editorHtml(html: string): string {
  const tpl = document.createElement('template');
  tpl.innerHTML = html;
  deferMediaSrc(tpl.content);
  return tpl.innerHTML;
}

/**
 * Replaces the editor's content with stored HTML: parsed into an inert <template>, media
 * deferred there, then attached and hydrated, so no bare name is ever requested.
 */
export function setEditorHtml(el: HTMLElement, html: string): void {
  const tpl = document.createElement('template');
  tpl.innerHTML = html;
  deferMediaSrc(tpl.content);
  el.replaceChildren(tpl.content);
  hydrateMedia(el);
}

const pending = new WeakSet<Element>();

/** Resolves media under `root` that carry a name in `data-loci-src` but no `src` yet to object URLs. */
export function hydrateMedia(root: HTMLElement): void {
  deferMediaSrc(root);
  for (const el of Array.from(root.querySelectorAll<HTMLElement>(MEDIA_SEL))) {
    const name = el.dataset.lociSrc;
    if (!name || el.hasAttribute('src') || el.dataset.lociMissing || pending.has(el)) continue;
    pending.add(el);
    void resolveMediaRef(name).then((url) => {
      pending.delete(el);
      if (el.dataset.lociSrc !== name) return;
      if (url) el.setAttribute('src', url);
      else el.dataset.lociMissing = '1';
    });
  }
}

/** The stored HTML of an editor root: names restored, editor-only attributes and sentinels removed. */
export function serializeEditor(root: HTMLElement, keepMarkers = false): string {
  // Work on an inert copy: setting `src` on an <img> owned by the live document starts a request
  // for the bare name even when the element is detached.
  const tpl = document.createElement('template');
  tpl.innerHTML = root.innerHTML;
  const clone = tpl.content;
  for (const el of Array.from(clone.querySelectorAll<HTMLElement>('[data-loci-src]'))) {
    el.setAttribute('src', el.dataset.lociSrc!);
    delete el.dataset.lociSrc;
    delete el.dataset.lociMissing;
  }
  for (const el of Array.from(clone.querySelectorAll<HTMLElement>('[data-loci-deco]'))) el.replaceWith(...Array.from(el.childNodes));
  let html = keepMarkers ? tpl.innerHTML : tpl.innerHTML.replace(new RegExp(`[${SEL_START}${SEL_END}]`, 'g'), '');
  if (html === '<br>' || html === '<div><br></div>') html = '';
  return html;
}

/** Stores a file in the collection and returns the name to reference. */
export async function storeMediaFile(file: File): Promise<string> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  return api.addMedia(pastedFileName(file), bytes);
}

/** HTML/text to insert for a stored media file. */
export function mediaMarkup(name: string, mime: string): { html?: string; text?: string } {
  // No `src` until hydrateMedia resolves the name (a bare src would be requested from the server).
  if (mime.startsWith('image/')) return { html: `<img data-loci-src="${name.replace(/&/g, '&amp;').replace(/"/g, '&quot;')}">` };
  return { text: `[sound:${name}]` };
}
