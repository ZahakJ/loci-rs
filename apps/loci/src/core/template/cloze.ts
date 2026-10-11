/**
 * Cloze deletions, ported from Anki's rslib/src/cloze.rs (research/anki-format.md §5.3).
 *
 * Syntax: `{{c1::text}}`, `{{c1::text::hint}}`, `{{c1,3::shared}}`, nested up to 10 levels,
 * overlapping via nesting. A `}}` inside a MathJax `\( … \)` / `\[ … \]` that started inside
 * a cloze does not close the cloze. Stray `}}` are literal text; clozes left open at the end
 * of the text are restored as literal text (Anki drops nothing silently either way).
 */
import { stripHtml } from '../text.ts';

export interface ClozeNode {
  kind: 'cloze';
  /** sorted, deduplicated 1-based ordinals (0 allowed but never generates a card) */
  ordinals: number[];
  nodes: ClozeTextOrCloze[];
  hint: string | null;
  /** raw opening marker, for restoring unclosed clozes */
  marker: string;
}
export type ClozeTextOrCloze = { kind: 'text'; text: string } | ClozeNode;

type Tok = { t: 'open'; ords: number[]; raw: string } | { t: 'close' } | { t: 'text'; text: string };

const OPEN_RE = /^\{\{c(\d+(?:,\d+)*)::/;
const MAX_NESTING = 10;

function tokenize(text: string): Tok[] {
  const out: Tok[] = [];
  let buf = '';
  let i = 0;
  // Depth of clozes opened so far and whether we are in MathJax opened inside a cloze.
  let depth = 0;
  let mathClose: string | null = null;
  const flush = () => { if (buf) { out.push({ t: 'text', text: buf }); buf = ''; } };
  while (i < text.length) {
    if (mathClose) {
      if (text.startsWith(mathClose, i)) { buf += mathClose; i += 2; mathClose = null; continue; }
      if (text.startsWith('{{c', i)) {
        const m = OPEN_RE.exec(text.slice(i, i + 64));
        if (m) {
          flush();
          out.push({ t: 'open', ords: parseOrds(m[1]!), raw: m[0] });
          depth++; i += m[0].length; continue;
        }
      }
      buf += text[i]; i++;
      continue;
    }
    if (text.startsWith('{{c', i)) {
      const m = OPEN_RE.exec(text.slice(i, i + 64));
      if (m) {
        flush();
        out.push({ t: 'open', ords: parseOrds(m[1]!), raw: m[0] });
        depth++; i += m[0].length; continue;
      }
    }
    if (text.startsWith('}}', i)) {
      flush(); out.push({ t: 'close' });
      if (depth > 0) depth--;
      i += 2; continue;
    }
    if (depth > 0 && text[i] === '\\' && (text[i + 1] === '(' || text[i + 1] === '[')) {
      mathClose = text[i + 1] === '(' ? '\\)' : '\\]';
      buf += text.slice(i, i + 2); i += 2; continue;
    }
    buf += text[i]; i++;
  }
  flush();
  return out;
}

function parseOrds(s: string): number[] {
  const set = new Set<number>();
  for (const p of s.split(',')) { const n = parseInt(p, 10); if (Number.isFinite(n) && n <= 65535) set.add(n); }
  return [...set].sort((a, b) => a - b);
}

/** Text that closes a parsed cloze verbatim (used to restore unclosed clozes). */
function nodesToSource(nodes: ClozeTextOrCloze[]): string {
  return nodes.map((n) => (n.kind === 'text' ? n.text : `${n.marker}${nodesToSource(n.nodes)}${n.hint !== null ? `::${n.hint}` : ''}}}`)).join('');
}

/** Parses text into text runs and (nested) clozes, exactly like Anki's `parse_text_with_clozes`. */
export function parseClozes(text: string): ClozeTextOrCloze[] {
  const open: ClozeNode[] = [];
  const output: ClozeTextOrCloze[] = [];
  const target = () => (open.length ? open[open.length - 1]!.nodes : output);
  const pushText = (arr: ClozeTextOrCloze[], s: string) => {
    if (!s) return;
    const last = arr[arr.length - 1];
    if (last && last.kind === 'text') last.text += s; else arr.push({ kind: 'text', text: s });
  };
  // Opens beyond MAX_NESTING are ignored by Anki, but their closing `}}` still pops; track them.
  let ignoredOpens = 0;
  for (const tok of tokenize(text)) {
    if (tok.t === 'open') {
      if (open.length < MAX_NESTING) open.push({ kind: 'cloze', ordinals: tok.ords, nodes: [], hint: null, marker: tok.raw });
      else ignoredOpens++;
    } else if (tok.t === 'text') {
      const cloze = open[open.length - 1];
      if (cloze) {
        let s = tok.text;
        const isIo = cloze.nodes.length === 0 && s.startsWith('image-occlusion:');
        const idx = isIo ? -1 : s.indexOf('::');
        if (idx >= 0) { cloze.hint = s.slice(idx + 2); s = s.slice(0, idx); }
        pushText(cloze.nodes, s);
      } else pushText(output, tok.text);
    } else {
      if (ignoredOpens > 0) { ignoredOpens--; continue; }
      const cloze = open.pop();
      if (cloze) target().push(cloze);
      else pushText(output, '}}');
    }
  }
  // Unclosed clozes become literal text again.
  while (open.length) {
    const c = open.pop()!;
    pushText(target(), c.marker + nodesToSource(c.nodes) + (c.hint !== null ? `::${c.hint}` : ''));
  }
  return output;
}

/** The revealed text of a cloze: its text plus the text of nested clozes, without markers. */
export function clozedText(c: ClozeNode): string {
  return c.nodes.map((n) => (n.kind === 'text' ? n.text : clozedText(n))).join('');
}

/**
 * Cloze markup rendered as the text a reader sees with every cloze revealed:
 * `{{c1::Paris::capital}}` → `Paris`, nested clozes flattened, hints dropped; image-occlusion
 * shapes (`{{c1::image-occlusion:rect:…}}`) carry no text and vanish. Used for search text and
 * browse cells. Text without `{{c` is returned unchanged.
 */
export function clozeToText(text: string): string {
  if (!text.includes('{{c')) return text;
  const walk = (ns: ClozeTextOrCloze[]): string => ns.map((n) => {
    if (n.kind === 'text') return n.text;
    if (imageOcclusionText(n) !== null) return '';
    return walk(n.nodes);
  }).join('');
  return walk(parseClozes(text));
}

const encodeAttr = (s: string) => s.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

/** Distinct cloze ordinals (>0) present in a text, nested included (Anki `cloze_numbers_in_string`). */
export function clozeNumbersInString(text: string): number[] {
  const set = new Set<number>();
  const walk = (ns: ClozeTextOrCloze[]) => {
    for (const n of ns) if (n.kind === 'cloze') { for (const o of n.ordinals) if (o > 0) set.add(o); walk(n.nodes); }
  };
  walk(parseClozes(text));
  return [...set].sort((a, b) => a - b);
}

/** IO cloze body (`image-occlusion:…`) if this cloze is an image-occlusion shape. */
export function imageOcclusionText(c: ClozeNode): string | null {
  const first = c.nodes[0];
  if (c.nodes.length === 1 && first && first.kind === 'text' && first.text.startsWith('image-occlusion:')) return first.text.slice('image-occlusion:'.length);
  return null;
}

export interface RevealOptions {
  /**
   * Adds `data-cloze-ord` (the active ordinal) and `data-cloze-index` (0-based order of the
   * active cloze within this text) to every active span, so the UI can reveal them one by one.
   */
  oneByOne?: boolean;
  /** Renders an image-occlusion cloze; default mimics Anki's `<div class=… data-shape=…>`. */
  renderOcclusion?: (body: string, active: boolean, ordinals: number[], question: boolean) => string;
}

function defaultOcclusionDiv(body: string, active: boolean, ordinals: number[], question: boolean): string {
  const [shape = '', ...props] = body.split(/(?<!\\):/);
  const cls = active ? (question ? 'cloze' : 'cloze-highlight') : 'cloze-inactive';
  let attrs = '';
  for (const p of props) {
    const eq = p.indexOf('=');
    if (eq <= 0) continue;
    const k = p.slice(0, eq).replace(/[^a-z0-9-]/gi, '');
    attrs += ` data-${k}="${encodeAttr(p.slice(eq + 1).replace(/\\(.)/g, '$1'))}"`;
  }
  return `<div class="${cls}" data-shape="${encodeAttr(shape)}" data-ordinal="${ordinals.join(',')}"${attrs}></div>`;
}

/**
 * Anki `reveal_cloze_text`: renders a field for card ordinal `ord` (1-based).
 * Returns '' when no cloze with that ordinal exists in the text.
 */
export function revealClozeText(text: string, ord: number, question: boolean, opts: RevealOptions = {}): string {
  let found = false;
  let activeIndex = 0;
  const io = opts.renderOcclusion ?? defaultOcclusionDiv;
  const out: string[] = [];
  const reveal = (n: ClozeTextOrCloze) => {
    if (n.kind === 'text') { out.push(n.text); return; }
    const active = n.ordinals.includes(ord);
    if (active) found = true;
    const ioBody = imageOcclusionText(n);
    if (ioBody !== null) { out.push(io(ioBody, active, n.ordinals, question)); return; }
    const ords = n.ordinals.join(',');
    const extra = active && opts.oneByOne ? ` data-cloze-ord="${ord}" data-cloze-index="${activeIndex++}"` : '';
    if (active && question) {
      out.push(`<span class="cloze" data-cloze="${encodeAttr(clozedText(n))}" data-ordinal="${ords}"${extra}>[${n.hint ?? '...'}]</span>`);
      return;
    }
    out.push(active ? `<span class="cloze" data-ordinal="${ords}"${extra}>` : `<span class="cloze-inactive" data-ordinal="${ords}">`);
    for (const c of n.nodes) reveal(c);
    out.push('</span>');
  };
  for (const n of parseClozes(text)) reveal(n);
  return found ? out.join('') : '';
}

const MATHJAX = /(\\[([])(.*?)(\\[)\]])/gs;
/** Anki `strip_html_inside_mathjax`: cloze spans inside `\(..\)`/`\[..\]` would break the TeX. */
export function stripHtmlInsideMathjax(text: string): string {
  return text.replace(MATHJAX, (_m, a: string, body: string, b: string) => a + stripHtml(body) + b);
}

/** The `cloze` template filter (Anki `cloze_filter`). */
export function clozeFilter(text: string, ord: number, question: boolean, opts: RevealOptions = {}): string {
  return stripHtmlInsideMathjax(revealClozeText(text, ord, question, opts));
}

function collectActive(nodes: ClozeTextOrCloze[], ord: number, question: boolean, out: string[]) {
  for (const n of nodes) {
    if (n.kind !== 'cloze') continue;
    if (n.ordinals.includes(ord)) out.push(question ? (n.hint ?? '...') : clozedText(n));
    collectActive(n.nodes, ord, question, out);
  }
}

/** The `cloze-only` filter (Anki `reveal_cloze_text_only`): active cloze texts (or hints) joined by ', '. */
export function revealClozeTextOnly(text: string, ord: number, question: boolean): string {
  const out: string[] = [];
  collectActive(parseClozes(text), ord, question, out);
  return out.join(', ');
}

/**
 * Expected answer for `{{type:cloze:F}}` (Anki `extract_cloze_for_typing`): the active cloze
 * texts; identical ones collapse to one, otherwise joined by ', '. Still HTML (caller strips).
 */
export function extractClozeForTyping(text: string, ord: number): string {
  const out: string[] = [];
  collectActive(parseClozes(text), ord, false, out);
  if (!out.length) return '';
  return out.every((s) => s === out[0]) ? out[0]! : out.join(', ');
}

// ───────────────────────── editor helpers ─────────────────────────

/** Next cloze number for "new cloze" (Ctrl+Shift+C): highest existing ordinal + 1, at least 1. */
export function nextClozeNumber(text: string): number {
  const nums = clozeNumbersInString(text);
  return nums.length ? nums[nums.length - 1]! + 1 : 1;
}

/** Highest existing ordinal (for "same cloze", Ctrl+Alt+Shift+C), or 1 when there is none. */
export function currentClozeNumber(text: string): number {
  const nums = clozeNumbersInString(text);
  return nums.length ? nums[nums.length - 1]! : 1;
}

/**
 * Wraps text[selectionStart, selectionEnd) as `{{cN::…}}`. Returns the new text and the
 * selection to restore: the wrapped content, or the caret inside an empty cloze.
 */
export function wrapCloze(text: string, selectionStart: number, selectionEnd: number, n: number): { text: string; selectionStart: number; selectionEnd: number } {
  const a = Math.max(0, Math.min(selectionStart, selectionEnd, text.length));
  const b = Math.min(text.length, Math.max(selectionStart, selectionEnd));
  const open = `{{c${Math.max(1, Math.floor(n))}::`;
  const inner = text.slice(a, b);
  const out = text.slice(0, a) + open + inner + '}}' + text.slice(b);
  return { text: out, selectionStart: a + open.length, selectionEnd: a + open.length + inner.length };
}

/** Plain text of a cloze answer for typing comparison (HTML stripped, entities decoded). */
export const clozeAnswerPlain = (html: string): string => stripHtml(html);
