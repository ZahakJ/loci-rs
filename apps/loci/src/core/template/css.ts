/**
 * Note-type CSS → stylesheet for the card's shadow root.
 *
 * Anki styles target `html`, `body`, `.card` and `.nightMode`/`.night_mode` on body. Inside our
 * shadow root the card is `<div class="card [nightMode night_mode]">`, so:
 *   `html`, `:root`            → `:host`
 *   `body`, `html body`        → `.card`
 *   `.nightMode .card`, `body.nightMode`, `.nightMode.card` → `.card.nightMode` (same for night_mode)
 *   `.card` and every other selector are kept (they already live inside the shadow root).
 * `@import`, `@charset`, `@namespace` are dropped; declarations using remote `url(http…)`/`url(//…)`
 * are dropped (privacy and offline); relative `url(name)` (collection media, e.g. `_font.ttf`
 * in `@font-face`) is rewritten through `mediaUrl`; `data:` URLs are kept.
 *
 * Loci owns the card's typography and colours (Settings → card font, size, alignment; light and
 * dark themes), so unless the note type opts out (`NotetypeConfig.useOwnStyling === true`):
 *   (a) rules of Anki's stock note-type CSS are dropped: `.card { font-family: arial; font-size:
 *       20px; line-height: 1.5; text-align: center; color: black; background-color: white }` (any
 *       subset of it, i.e. older and newer Anki versions), `.cloze { font-weight: bold; color: blue }`,
 *       `.nightMode .cloze { color: lightblue }`, and the image-occlusion template's
 *       `#image-occlusion-container` / `#image-occlusion-canvas` rules (Loci draws IO itself);
 *   (b) in the remaining rules aimed at the card root (`.card`, `body`, `html`, `.card.nightMode`, …)
 *       the declarations background, background-color, color, font, font-family and font-size are
 *       removed; every other selector and declaration is kept.
 * Night mode: the UI adds the class `nightMode` (and `night_mode`) to the card root element
 * (`<div class="card">`) when it renders in the dark theme, so Anki rules such as
 * `.nightMode .cloze` (scoped to `.card.nightMode .cloze`) keep working.
 */
export interface CardCssOptions {
  /** The note type's `config.useOwnStyling`: keep its colours and fonts (skip (a) and (b)). */
  ownStyling?: boolean;
}

type MediaUrl = (name: string) => string;

function stripComments(css: string): string {
  return css.replace(/\/\*[\s\S]*?\*\//g, '');
}

/** Index of the `}` matching the `{` at `open`, honouring strings; css.length if unbalanced. */
function matchBrace(css: string, open: number): number {
  let depth = 0;
  for (let i = open; i < css.length; i++) {
    const ch = css[i];
    if (ch === '"' || ch === "'") { const q = ch; i++; while (i < css.length && css[i] !== q) { if (css[i] === '\\') i++; i++; } continue; }
    if (ch === '{') depth++;
    else if (ch === '}') { depth--; if (depth === 0) return i; }
  }
  return css.length;
}

/** Splits on commas at parenthesis depth 0. */
function splitTopLevel(s: string, sep: string): string[] {
  const out: string[] = [];
  let depth = 0, cur = '', quote: string | null = null;
  for (let i = 0; i < s.length; i++) {
    const ch = s[i]!;
    if (quote) { cur += ch; if (ch === '\\') { cur += s[++i] ?? ''; } else if (ch === quote) quote = null; continue; }
    if (ch === '"' || ch === "'") { quote = ch; cur += ch; continue; }
    if (ch === '(' || ch === '[') depth++;
    else if (ch === ')' || ch === ']') depth--;
    if (ch === sep && depth === 0) { out.push(cur); cur = ''; continue; }
    cur += ch;
  }
  out.push(cur);
  return out;
}

/** Rewrites one selector for the shadow root (see file header). */
export function scopeSelector(sel: string): string {
  let s = sel.trim();
  if (!s) return s;
  s = s.replace(/^(?:html\s+)?body\.(nightMode|night_mode)(?:\s+\.card\b)?/, '.card.$1');
  s = s.replace(/^(?:html\s*)?\.(nightMode|night_mode)(?:\s+body)?\s+\.card\b/, '.card.$1');
  s = s.replace(/^\.(nightMode|night_mode)\.card\b/, '.card.$1');
  s = s.replace(/^\.card\.(nightMode|night_mode)\b/, '.card.$1');
  s = s.replace(/^(?:html\s+)?body\b/, '.card');
  s = s.replace(/^html\b/, ':host');
  s = s.replace(/^:root\b/, ':host');
  // `.card .card` can appear after `body .card` → `.card .card`; collapse.
  s = s.replace(/^\.card\s+\.card\b/, '.card');
  return s;
}

const URL_RE = /url\(\s*(?:"([^"]*)"|'([^']*)'|([^)\s]*))\s*\)/gi;

/** Rewrites url() references in one value item; null when it loads something remote. */
function rewriteUrls(item: string, mediaUrl?: MediaUrl): string | null {
  let remote = false;
  const out = item.replace(URL_RE, (m, dq?: string, sq?: string, bare?: string) => {
    const ref = (dq ?? sq ?? bare ?? '').trim();
    if (/^(?:https?:)?\/\//i.test(ref) || /^(?:javascript|vbscript):/i.test(ref)) { remote = true; return m; }
    if (/^(?:data|blob):/i.test(ref) || !ref || ref.startsWith('#') || !mediaUrl) return m;
    let name = ref;
    try { name = decodeURIComponent(ref); } catch { /* literal % */ }
    return `url("${mediaUrl(name).replace(/["\\]/g, '\\$&')}")`;
  });
  return remote ? null : out;
}

/**
 * Processes a declaration block: in comma-separated values (font-face `src`, layered
 * backgrounds) remote items are removed; a declaration left with nothing is dropped.
 */
function processDeclarations(block: string, mediaUrl?: MediaUrl): string {
  const out: string[] = [];
  for (const decl of splitTopLevel(block, ';')) {
    if (!decl.trim()) continue;
    if (!/url\(/i.test(decl)) { out.push(decl.trim()); continue; }
    const colon = decl.indexOf(':');
    if (colon < 0) continue;
    const items = splitTopLevel(decl.slice(colon + 1), ',').map((i) => rewriteUrls(i, mediaUrl)).filter((i): i is string => i !== null);
    if (items.length) out.push(`${decl.slice(0, colon).trim()}: ${items.map((i) => i.trim()).join(', ')}`);
  }
  return out.length ? ` ${out.join('; ')}; ` : ' ';
}

const normDecl = (d: string): string => {
  const colon = d.indexOf(':');
  if (colon < 0) return d.trim().toLowerCase();
  return `${d.slice(0, colon).trim().toLowerCase()}:${d.slice(colon + 1).trim().toLowerCase().replace(/\s+/g, ' ')}`;
};
const declProp = (d: string): string => d.slice(0, Math.max(0, d.indexOf(':'))).trim().toLowerCase();
const normSel = (sel: string): string => sel.trim().toLowerCase().replace(/\s+/g, ' ');

/** Anki's stock rules: selector → the declarations it may carry (a rule with a subset is stock). */
const STOCK_RULES: Array<{ sel: string; allowed: Set<string>; required: string }> = [
  {
    sel: '.card',
    allowed: new Set(['font-family:arial', 'font-size:20px', 'line-height:1.5', 'text-align:center', 'color:black', 'background-color:white']),
    required: 'font-family:arial',
  },
  { sel: '.cloze', allowed: new Set(['font-weight:bold', 'color:blue']), required: 'color:blue' },
  { sel: '.nightmode .cloze', allowed: new Set(['color:lightblue']), required: 'color:lightblue' },
  { sel: '.night_mode .cloze', allowed: new Set(['color:lightblue']), required: 'color:lightblue' },
];
const IO_STOCK_SELECTOR = /^#image-occlusion-(?:container|canvas)\b/;

/** True for a rule of Anki's stock note-type CSS (see the file header). */
function isStockRule(selectors: string[], decls: string[]): boolean {
  const sels = selectors.map(normSel).filter(Boolean);
  if (!sels.length) return false;
  if (sels.every((x) => IO_STOCK_SELECTOR.test(x))) return true;
  if (sels.length !== 1) return false;
  const rule = STOCK_RULES.find((r) => r.sel === sels[0]);
  if (!rule) return false;
  const norm = decls.map(normDecl).filter(Boolean);
  return norm.length > 0 && norm.includes(rule.required) && norm.every((d) => rule.allowed.has(d));
}

/** Selectors (after scoping) that style the card root itself, whose colours and fonts Loci owns. */
const CARD_ROOT = /^(?:\.card(?:\.(?:nightMode|night_mode))*|:host)$/;
const OWNED_PROPS = new Set(['background', 'background-color', 'color', 'font', 'font-family', 'font-size']);

function scopeBlockList(css: string, mediaUrl?: MediaUrl, opts: CardCssOptions = {}): string {
  let out = '';
  let i = 0;
  while (i < css.length) {
    // skip whitespace
    while (i < css.length && /\s/.test(css[i]!)) i++;
    if (i >= css.length) break;
    if (css[i] === '}') { i++; continue; } // stray
    const braceAt = css.indexOf('{', i);
    const semiAt = css.indexOf(';', i);
    if (css[i] === '@') {
      const nameMatch = /^@([\w-]+)/.exec(css.slice(i));
      const name = (nameMatch?.[1] ?? '').toLowerCase();
      if (semiAt >= 0 && (braceAt < 0 || semiAt < braceAt)) {
        // statement at-rule
        const stmt = css.slice(i, semiAt + 1);
        if (!['import', 'charset', 'namespace'].includes(name)) out += stmt + '\n';
        i = semiAt + 1;
        continue;
      }
      if (braceAt < 0) break;
      const close = matchBrace(css, braceAt);
      const prelude = css.slice(i, braceAt).trim();
      const body = css.slice(braceAt + 1, close);
      if (['media', 'supports', 'container', 'layer', 'document'].includes(name)) {
        out += `${prelude} {\n${scopeBlockList(body, mediaUrl, opts)}}\n`;
      } else if (name === 'font-face' || name === 'page' || name === 'property' || name === 'counter-style' || name === 'font-feature-values') {
        out += `${prelude} {${processDeclarations(body, mediaUrl)}}\n`;
      } else {
        out += `${prelude} {${body}}\n`; // keyframes etc.
      }
      i = close + 1;
      continue;
    }
    if (braceAt < 0) break;
    const close = matchBrace(css, braceAt);
    const rawSelectors = splitTopLevel(css.slice(i, braceAt), ',');
    const selectors = rawSelectors.map(scopeSelector).filter(Boolean);
    let block = css.slice(braceAt + 1, close);
    i = close + 1;
    if (!opts.ownStyling) {
      const decls = splitTopLevel(block, ';').filter((d) => d.trim());
      if (isStockRule(rawSelectors, decls)) continue;
      if (selectors.length && selectors.every((x) => CARD_ROOT.test(x.trim()))) {
        const kept = decls.filter((d) => !OWNED_PROPS.has(declProp(d)));
        if (!kept.length) continue;
        block = kept.join(';');
      }
    }
    const body = processDeclarations(block, mediaUrl);
    if (selectors.length) out += `${selectors.join(', ')} {${body}}\n`;
  }
  return out;
}

/**
 * Scopes note-type CSS for the card shadow root. `mediaUrl` resolves relative url() references;
 * unless `opts.ownStyling`, Anki's stock rules and the card root's colours and fonts are removed.
 */
export function scopeCardCss(css: string, mediaUrl?: MediaUrl, opts: CardCssOptions = {}): string {
  return scopeBlockList(stripComments(css), mediaUrl, opts);
}

/**
 * Loci's base card stylesheet (internals §6, §15), placed before the note type's CSS in the shadow
 * root. Driven by variables the host sets: --loci-card-font, --loci-card-size, --loci-ink,
 * --loci-accent, --loci-cloze, --loci-muted, --loci-line, --loci-surface (plus optional
 * --loci-card-align, --loci-mono, --loci-good/--loci-again/--loci-hard and --loci-io-fill/-stroke).
 */
export const LOCI_CARD_BASE_CSS = `
:host {
  display: block;
  color: var(--loci-ink);
  --_ink: var(--loci-ink, oklch(22% .02 260));
  --_muted: var(--loci-muted, oklch(50% .02 260));
  --_line: var(--loci-line, oklch(88% .01 85));
  --_surface: var(--loci-surface, oklch(99% .004 85));
  --_accent: var(--loci-accent, oklch(62% .13 70));
  --_cloze: var(--loci-cloze, var(--_accent));
  --_mono: var(--loci-mono, "JetBrains Mono Variable", "JetBrains Mono", ui-monospace, "SFMono-Regular", Menlo, Consolas, monospace);
  --_good: var(--loci-good, oklch(66% .14 150));
  --_again: var(--loci-again, oklch(62% .17 20));
  --_hard: var(--loci-hard, oklch(72% .14 65));
}
.card {
  font-family: var(--loci-card-font, "Newsreader Variable", "Newsreader", Georgia, var(--loci-arabic-font, "Noto Naskh Arabic", serif));
  font-size: var(--loci-card-size, 1.375rem);
  line-height: 1.45;
  color: var(--_ink);
  background: transparent;
  text-align: var(--loci-card-align, start);
  overflow-wrap: break-word;
  font-variant-numeric: oldstyle-nums proportional-nums;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
  unicode-bidi: plaintext;
}
.card :where(p, div, li, dd, dt, td, th, blockquote, h1, h2, h3, h4, h5, h6, figcaption) { unicode-bidi: plaintext; }
.card :where([dir]) { unicode-bidi: isolate; }
.card[dir="rtl"], .card :where([dir="rtl"], :lang(ar), :lang(fa), :lang(ur)) { line-height: 1.7; }
.card[dir="rtl"] { font-size: calc(var(--loci-card-size, 1.375rem) * 1.15); }
.card :where(:lang(ar), :lang(fa), :lang(ur)) { font-family: var(--loci-arabic-font, "Noto Naskh Arabic", serif); font-size: 1.15em; }
.card :where(p) { margin: 0 0 .75em; }
.card :where(p:last-child) { margin-block-end: 0; }
.card :where(h1, h2, h3, h4) { font-weight: 600; line-height: 1.2; margin: .4em 0 .5em; }
.card :where(h1) { font-size: 1.5em; } .card :where(h2) { font-size: 1.3em; } .card :where(h3) { font-size: 1.15em; }
.card :where(b, strong) { font-weight: 600; }
.card :where(ul, ol) { margin: .25em 0 .75em; padding-inline-start: 1.4em; text-align: start; }
.card :where(li) { margin: .15em 0; }
.card :where(li::marker) { color: var(--_muted); }
.card :where(a) { color: var(--_accent); text-decoration-thickness: 1px; text-underline-offset: .18em; }
.card :where(blockquote) { margin: .75em 0; padding-inline-start: 1em; border-inline-start: 2px solid color-mix(in oklch, var(--_muted) 45%, transparent); color: var(--_muted); }
.card :where(small, sub, sup) { font-size: .75em; }
.card :where(mark) { background: color-mix(in oklch, var(--_accent) 22%, transparent); color: inherit; padding: 0 .1em; border-radius: 2px; }

/* the hairline between question and answer (Anki's <hr id=answer>) */
.card :where(hr) { border: 0; height: 1px; background: var(--_line); margin: 1.25em 0; }
.card hr#answer { margin: 1.5em auto; transform-origin: center; }

/* media */
.card :where(img, video, svg, canvas) { max-width: 100%; height: auto; }
.card :where(img) { border-radius: 6px; vertical-align: middle; }
.card :where(img.latex) { border-radius: 0; }
.card :where(audio) { display: none; }

/* tables */
.card :where(table) { border-collapse: collapse; margin: .75em auto; font-size: .9em; max-width: 100%; display: block; overflow-x: auto; }
.card :where(th, td) { border: 1px solid var(--_line); padding: .35em .6em; vertical-align: top; text-align: start; }
.card :where(th) { font-weight: 600; background: color-mix(in oklch, var(--_line) 35%, transparent); }

/* code */
.card :where(code, kbd, samp, pre) { font-family: var(--_mono); font-size: .82em; }
.card :where(code, kbd) { background: color-mix(in oklch, var(--_line) 45%, transparent); padding: .08em .3em; border-radius: 4px; }
.card :where(kbd) { border: 1px solid var(--_line); border-bottom-width: 2px; }
.card :where(pre) { text-align: start; direction: ltr; background: color-mix(in oklch, var(--_line) 25%, var(--_surface)); border: 1px solid var(--_line); border-radius: 10px; padding: .8em 1em; overflow-x: auto; line-height: 1.5; white-space: pre; }
.card :where(pre code) { background: none; padding: 0; font-size: 1em; }

/* ruby (furigana) */
.card :where(ruby rt) { font-size: .5em; color: var(--_muted); }

/* cloze: accent colour, never bold blue */
.card .cloze { color: var(--_cloze); font-weight: inherit; }
.card span.cloze[data-cloze] { font-family: var(--_mono); font-size: .85em; letter-spacing: .02em; padding: 0 .2em; border-radius: 4px; background: color-mix(in oklch, var(--_cloze) 12%, transparent); }
.card .cloze-inactive { color: inherit; }
.card .cloze[data-cloze-index].loci-hidden { color: transparent; background: color-mix(in oklch, var(--_cloze) 14%, transparent); border-radius: 4px; cursor: pointer; }
.card.nightMode .cloze, .card.night_mode .cloze { color: var(--_cloze); }

/* night mode: black text and white highlights pasted into fields as inline styles (common in Anki
   collections) follow the theme instead of painting black-on-dark or white panels */
.card.nightMode :is([style*="color: black" i], [style*="color:black" i], [style*="color: rgb(0, 0, 0)"], [style*="color:rgb(0,0,0)"], [style*="color: #000;" i], [style*="color:#000;" i], [style$="color: #000" i], [style$="color:#000" i], [style*="color: #000000" i], [style*="color:#000000" i], [style*="color: windowtext" i]) { color: var(--_ink) !important; }
.card.nightMode :is([style*="background-color: white" i], [style*="background-color:white" i], [style*="background-color: rgb(255, 255, 255)"], [style*="background-color:rgb(255,255,255)"], [style*="background-color: #fff" i], [style*="background-color:#fff" i], [style*="background: white" i], [style*="background:white" i], [style*="background: rgb(255, 255, 255)"], [style*="background: #fff" i], [style*="background-color: window" i]) { background-color: transparent !important; }
.card.nightMode :is([bgcolor="white" i], [bgcolor="#ffffff" i], [bgcolor="#fff" i]) { background-color: transparent; }
.card.nightMode font[color="black" i], .card.nightMode font[color="#000000" i], .card.nightMode font[color="#000" i] { color: var(--_ink); }

/* maths (KaTeX output goes inside .loci-math) */
.card .loci-math { font-size: 1em; }
.card .loci-math[data-display="1"] { display: block; margin: .6em 0; text-align: center; overflow-x: auto; overflow-y: hidden; padding: .1em 0; }
.card .loci-math .katex { font-size: 1.08em; }
.card .loci-math[data-error] { font-family: var(--_mono); font-size: .8em; color: var(--_muted); white-space: pre-wrap; }

/* hints */
.card a.hint { font-family: inherit; font-size: .8em; color: var(--_accent); text-decoration: none; border-bottom: 1px dashed currentColor; cursor: pointer; }
.card div.hint { margin-top: .4em; color: var(--_muted); font-size: .9em; }

/* typed answers (Anki class names) */
.card .loci-type { display: block; margin: .75em 0; }
.card #typeans { display: inline-block; font-family: var(--_mono); font-size: .85em; line-height: 1.7; text-align: start; padding: .5em .75em; border-radius: 10px; border: 1px solid var(--_line); background: var(--_surface); white-space: pre-wrap; }
.card .typeGood { color: var(--_good); background: color-mix(in oklch, var(--_good) 12%, transparent); }
.card .typeBad { color: var(--_again); background: color-mix(in oklch, var(--_again) 12%, transparent); text-decoration: line-through; text-decoration-thickness: 1px; }
.card .typeMissed { color: var(--_hard); background: color-mix(in oklch, var(--_hard) 12%, transparent); }
.card #typearrow { color: var(--_muted); }

/* Anki's stock image-occlusion toggle button runs Anki-only script; Loci reveals masks itself. */
.card button#toggle, .card .toggle-masks { display: none; }
/* stray buttons in card HTML get the app's quiet button look */
.card button:not(.loci-audio) { font: inherit; font-size: 0.8em; padding: 0.3em 0.9em; border: 1px solid var(--loci-line); border-radius: 999px; background: transparent; color: var(--loci-muted); cursor: pointer; }
/* audio button */
.card .loci-audio {
  display: inline-grid; place-items: center; vertical-align: middle;
  inline-size: 2.25rem; block-size: 2.25rem; margin: .1em .25em; padding: 0;
  border: 1px solid var(--_line); border-radius: 999px; background: var(--_surface); color: var(--_accent);
  cursor: pointer; transition: border-color 120ms cubic-bezier(.2,.8,.2,1), background-color 120ms cubic-bezier(.2,.8,.2,1);
}
.card .loci-audio::before { content: ""; inline-size: 0; block-size: 0; margin-inline-start: 3px; border-block: 6px solid transparent; border-inline-start: 10px solid currentColor; }
.card .loci-audio:hover { border-color: var(--_accent); }
.card .loci-audio:focus-visible { outline: 2px solid var(--_accent); outline-offset: 2px; }
.card .loci-audio[data-playing]::before { border: 0; inline-size: 10px; block-size: 10px; margin: 0; background: currentColor; border-radius: 2px; }

/* image occlusion masks (SVG overlay drawn by the UI) */
.card .loci-io { position: relative; display: inline-block; max-width: 100%; line-height: 0; }
.card .loci-io > svg { position: absolute; inset: 0; width: 100%; height: 100%; }
.card .loci-io-mask { fill: var(--loci-io-fill, oklch(86% .02 75)); stroke: var(--loci-io-stroke, oklch(58% .03 70)); stroke-width: 1.5px; vector-effect: non-scaling-stroke; }
.card .loci-io-mask.is-active { fill: color-mix(in oklch, var(--_accent) 70%, var(--loci-io-fill, oklch(86% .02 75))); stroke: var(--_accent); }
.card .loci-io-mask.is-revealed { fill: transparent; stroke: var(--_accent); stroke-dasharray: 4 3; }
.card.nightMode .loci-io-mask, .card.night_mode .loci-io-mask { fill: var(--loci-io-fill, oklch(40% .02 70)); stroke: var(--loci-io-stroke, oklch(62% .03 70)); }
.card .loci-io-label { fill: var(--_ink); font-family: inherit; }

/* template problems */
.card .loci-template-error { font-family: var(--loci-ui-font, system-ui, sans-serif); font-size: .8em; color: var(--_muted); border: 1px dashed var(--_line); border-radius: 10px; padding: .75em 1em; text-align: start; }

@media (prefers-reduced-motion: reduce) { .card .loci-audio { transition: none; } }
`;
