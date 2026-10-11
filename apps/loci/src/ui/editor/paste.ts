/**
 * Paste cleaning for the field editor (DESIGN §10: "paste strips styles by default").
 *
 * Pure string → string (no DOM), so it runs in tests and never executes pasted markup.
 * Keeps: b i u sub sup br img ul ol li a code pre (strong/em/s-like tags are mapped onto b/i).
 * Spans survive only when they carry `dir` or `lang`; inline `font-weight: bold|600+` and
 * `font-style: italic` (Google Docs, Word) become <b>/<i>. Block elements (p, div, h1–h6, tr,
 * blockquote) become their content followed by a line break. Everything else is unwrapped;
 * script/style/head/title/template/comments/Office namespaced tags are dropped with content.
 */

const KEEP = new Set(['b', 'i', 'u', 'sub', 'sup', 'br', 'img', 'ul', 'ol', 'li', 'a', 'code', 'pre']);
const RENAME: Record<string, string> = { strong: 'b', em: 'i', ins: 'u', tt: 'code', kbd: 'code', samp: 'code' };
const BLOCK = new Set(['p', 'div', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'tr', 'blockquote', 'section', 'article', 'header', 'footer', 'figure', 'figcaption', 'dt', 'dd', 'table', 'address']);
const DROP_WITH_CONTENT = new Set(['script', 'style', 'head', 'title', 'template', 'noscript', 'iframe', 'object', 'embed', 'svg', 'math', 'xml', 'meta', 'link', 'button', 'select', 'textarea']);
const VOID = new Set(['br', 'img', 'hr', 'meta', 'link', 'input', 'col', 'wbr', 'area', 'base', 'source', 'track', 'embed']);

const escAttr = (s: string) => s.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const escText = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

/** Parses `name="v" name='v' name=v name` into a map (lower-cased names). */
export function parseAttributes(src: string): Map<string, string> {
  const out = new Map<string, string>();
  const re = /([^\s"'<>/=]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+)))?/g;
  for (const m of src.matchAll(re)) out.set(m[1]!.toLowerCase(), m[2] ?? m[3] ?? m[4] ?? '');
  return out;
}

function safeHref(href: string): string | null {
  const h = href.trim();
  return /^(?:https?:|mailto:|#)/i.test(h) ? h : null;
}
function safeSrc(src: string): string | null {
  const s = src.trim();
  if (!s || /^(?:javascript|vbscript|file):/i.test(s)) return null;
  if (/^data:/i.test(s) && !/^data:image\/(?:png|jpe?g|gif|webp|avif|svg\+xml)[;,]/i.test(s)) return null;
  return s;
}

/** Styles that carry meaning we keep as tags. */
function styleMarks(style: string): { bold: boolean; italic: boolean; underline: boolean; normalWeight: boolean } {
  const s = style.toLowerCase();
  const w = /font-weight\s*:\s*([a-z0-9]+)/.exec(s)?.[1];
  const bold = w === 'bold' || w === 'bolder' || (w !== undefined && /^\d+$/.test(w) && Number(w) >= 600);
  const normalWeight = w === 'normal' || w === '400' || w === 'lighter';
  const italic = /font-style\s*:\s*italic/.test(s);
  const underline = /text-decoration[a-z-]*\s*:[^;]*underline/.test(s);
  return { bold, italic, underline, normalWeight };
}

interface Open { name: string; emitted: string[] }

/** Cleans pasted HTML. Output is always well-formed for the tags it emits. */
export function cleanPastedHtml(input: string): string {
  // Office fragments: keep only what is between the fragment markers when present.
  let html = input;
  const start = html.indexOf('<!--StartFragment-->');
  const end = html.indexOf('<!--EndFragment-->');
  if (start >= 0 && end > start) html = html.slice(start + 20, end);
  html = html.replace(/<!--[\s\S]*?-->/g, '').replace(/<!\[CDATA\[[\s\S]*?\]\]>/g, '').replace(/<!doctype[^>]*>/gi, '');

  const out: string[] = [];
  const stack: Open[] = [];
  let dropDepth = 0;
  let dropName = '';
  const tagRe = /<(\/?)([a-zA-Z][\w:.-]*)((?:[^>"']|"[^"]*"|'[^']*')*)>/g;
  let last = 0;
  const pushText = (text: string) => {
    if (dropDepth > 0 || !text) return;
    // Pasted HTML whitespace is insignificant except inside <pre>.
    const inPre = stack.some((o) => o.name === 'pre');
    out.push(inPre ? text : text.replace(/[\r\n\t]+/g, ' '));
  };
  const lineBreak = () => {
    // Avoid leading and doubled breaks produced by nested blocks.
    let i = out.length - 1;
    while (i >= 0 && /^\s*$/.test(out[i]!)) i--;
    if (i < 0) return;
    const prev = out[i]!;
    if (prev === '<br>' || /^<(?:ul|ol|li|\/ul|\/ol|\/li|pre|\/pre)\b/.test(prev)) return;
    out.push('<br>');
  };

  for (const m of html.matchAll(tagRe)) {
    pushText(html.slice(last, m.index));
    last = m.index! + m[0].length;
    const closing = m[1] === '/';
    const raw = m[2]!.toLowerCase();
    const selfClosing = /\/\s*$/.test(m[3] ?? '');
    if (dropDepth > 0) {
      if (raw === dropName) dropDepth += closing ? -1 : selfClosing ? 0 : 1;
      continue;
    }
    if (DROP_WITH_CONTENT.has(raw) || raw.includes(':')) {
      if (!closing && !selfClosing && !VOID.has(raw)) { dropDepth = 1; dropName = raw; }
      continue;
    }
    const name = RENAME[raw] ?? raw;
    if (closing) {
      if (BLOCK.has(name)) { closeTo(name); lineBreak(); continue; }
      closeTo(name);
      continue;
    }
    const attrs = parseAttributes(m[3] ?? '');
    if (name === 'br') { out.push('<br>'); continue; }
    if (name === 'hr') { lineBreak(); continue; }
    if (name === 'img') {
      const src = safeSrc(attrs.get('src') ?? '');
      if (src) out.push(`<img src="${escAttr(src)}"${attrs.get('alt') ? ` alt="${escAttr(attrs.get('alt')!)}"` : ''}>`);
      continue;
    }
    if (VOID.has(name)) continue;
    if (BLOCK.has(name)) { lineBreak(); stack.push({ name, emitted: [] }); continue; }
    const dirLang = ['dir', 'lang'].filter((k) => attrs.has(k)).map((k) => ` ${k}="${escAttr(attrs.get(k)!)}"`).join('');
    const marks = styleMarks(attrs.get('style') ?? '');
    const emitted: string[] = [];
    if (name === 'b' && marks.normalWeight) {
      // Google Docs wraps everything in <b style="font-weight:normal">.
    } else if (KEEP.has(name)) {
      if (name === 'a') {
        const href = safeHref(attrs.get('href') ?? '');
        if (href) { out.push(`<a href="${escAttr(href)}">`); emitted.push('a'); }
      } else if (name === 'ol' || name === 'ul' || name === 'li' || name === 'pre' || name === 'code' || name === 'b' || name === 'i' || name === 'u' || name === 'sub' || name === 'sup') {
        out.push(`<${name}${dirLang}>`); emitted.push(name);
      }
    } else if (name === 'span' && dirLang) {
      out.push(`<span${dirLang}>`); emitted.push('span');
    }
    if (marks.bold && !emitted.includes('b')) { out.push('<b>'); emitted.push('b'); }
    if (marks.italic && !emitted.includes('i')) { out.push('<i>'); emitted.push('i'); }
    if (marks.underline && !emitted.includes('u')) { out.push('<u>'); emitted.push('u'); }
    if (selfClosing) { for (const e of emitted.reverse()) out.push(`</${e}>`); continue; }
    stack.push({ name, emitted });
  }
  pushText(html.slice(last));
  closeTo(null);

  function closeTo(name: string | null) {
    // Close up to and including the nearest open `name` (or everything when null).
    const idx = name === null ? 0 : findLast(stack, (o) => o.name === name);
    if (idx < 0) return;
    while (stack.length > idx) {
      const o = stack.pop()!;
      for (const e of [...o.emitted].reverse()) out.push(`</${e}>`);
    }
  }

  let result = out.join('');
  // Tidy: trailing breaks, empty inline wrappers, runs of 3+ breaks, &nbsp; runs.
  result = result.replace(/<(b|i|u|sub|sup|code|span)(?: [^>]*)?><\/\1>/g, '');
  result = result.replace(/(?:<br>\s*){3,}/g, '<br><br>');
  result = result.replace(/^(?:\s|<br>)+|(?:\s|<br>)+$/g, '');
  return result;
}

function findLast<T>(arr: T[], pred: (t: T) => boolean): number {
  for (let i = arr.length - 1; i >= 0; i--) if (pred(arr[i]!)) return i;
  return -1;
}

/** Plain text → HTML for a field (shift+paste): escapes and turns newlines into <br>. */
export function plainTextToHtml(text: string): string {
  return escText(text.replace(/\r\n?/g, '\n')).replace(/\n/g, '<br>');
}

/** Extension for a pasted/dropped file's MIME type. */
export function extensionFor(mime: string): string {
  const m = mime.toLowerCase();
  const map: Record<string, string> = {
    'image/png': 'png', 'image/jpeg': 'jpg', 'image/gif': 'gif', 'image/webp': 'webp', 'image/svg+xml': 'svg', 'image/avif': 'avif',
    'audio/mpeg': 'mp3', 'audio/ogg': 'ogg', 'audio/wav': 'wav', 'audio/webm': 'webm', 'audio/mp4': 'm4a', 'audio/x-m4a': 'm4a', 'audio/flac': 'flac',
    'video/mp4': 'mp4', 'video/webm': 'webm',
  };
  return map[m] ?? (m.split('/')[1]?.replace(/[^a-z0-9]/g, '') || 'bin');
}

/** A media filename for a pasted blob without a useful name: `paste-<timestamp>.<ext>`. */
export function pastedFileName(file: { name?: string; type: string }, now: number = Date.now()): string {
  const name = (file.name ?? '').trim();
  if (name && name !== 'image.png' && name !== 'blob') return name;
  return `paste-${now.toString(36)}.${extensionFor(file.type)}`;
}
