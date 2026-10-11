/**
 * Typed-answer comparison ({{type:Field}}), modelled on Anki's rslib/src/typeanswer.rs.
 *
 * Both strings are normalised (NFC, whitespace collapsed; the expected text is HTML-stripped
 * with [sound:] tags removed), then compared per user-perceived character (a base code point
 * plus its combining marks) with optional case folding and diacritic stripping. The diff is a
 * longest-common-subsequence alignment; the HTML mirrors Anki's markup so existing deck CSS
 * keeps working:
 *   <code id=typeans>typed line<br><span id=typearrow>&darr;</span><br>expected line</code>
 * with `typeGood` (matching), `typeBad` (wrong/extra typed chars) and `typeMissed` (missing chars;
 * shown as '-' in the typed line).
 */
import { stripHtml } from '../text';

export interface TypedCompareOptions {
  /** compare case-insensitively (default false, like Anki) */
  ignoreCase?: boolean;
  /** strip combining marks before comparing, like Anki's `nc` modifier (default false) */
  ignoreDiacritics?: boolean;
}

export type TypedDiffOp = 'equal' | 'replace' | 'delete' | 'insert';
/** One aligned run. `delete` = extra typed text; `insert` = expected text that was not typed. */
export interface TypedSegment { op: TypedDiffOp; typed: string; expected: string }

export interface TypedResult {
  correct: boolean;
  segments: TypedSegment[];
  /** Anki-compatible HTML (<code id=typeans>…) */
  html: string;
  /** normalised strings that were compared (display forms, not folded) */
  expected: string;
  typed: string;
}

/** Anki `html_to_text_line` + av-tag stripping: one line of plain text from field HTML. */
export function expectedTextFromHtml(html: string): string {
  const s = html
    .replace(/\[sound:[^\]]*\]/g, '')
    .replace(/<br\s*\/?>|<\/?div[^>]*>|<\/?p[^>]*>|\n/gi, ' ');
  return normalizeTypedText(stripHtml(s));
}

/** NFC + trim + collapse runs of whitespace (incl. NBSP) to one space. */
export function normalizeTypedText(s: string): string {
  return s.normalize('NFC').replace(/[\s ]+/g, ' ').trim();
}

/** Splits into user-perceived characters: base code point plus following combining marks. */
const UNIT = /\P{M}\p{M}*|\p{M}+/gu;
function units(s: string): string[] { return s.match(UNIT) ?? []; }

function foldKey(u: string, o: TypedCompareOptions): string {
  let k = u.normalize('NFD');
  if (o.ignoreDiacritics) k = k.replace(/\p{M}/gu, '');
  if (o.ignoreCase) k = k.toLowerCase();
  return k.normalize('NFC');
}

/** Largest table the LCS is allowed to build (cells); beyond it the middle is one replace. */
const MAX_CELLS = 4_000_000;

/** LCS alignment of key arrays → opcodes over index ranges (difflib-style). */
function align(a: string[], b: string[]): Array<{ op: TypedDiffOp; a0: number; a1: number; b0: number; b1: number }> {
  let pre = 0;
  while (pre < a.length && pre < b.length && a[pre] === b[pre]) pre++;
  let suf = 0;
  while (suf < a.length - pre && suf < b.length - pre && a[a.length - 1 - suf] === b[b.length - 1 - suf]) suf++;
  const n = a.length - pre - suf, m = b.length - pre - suf;
  // Pairs of (i in a, j in b) that match, in order.
  const pairs: Array<[number, number]> = [];
  for (let i = 0; i < pre; i++) pairs.push([i, i]);
  if (n > 0 && m > 0 && (n + 1) * (m + 1) <= MAX_CELLS) {
    const w = m + 1;
    const dp = new Uint32Array((n + 1) * w);
    for (let i = n - 1; i >= 0; i--) {
      for (let j = m - 1; j >= 0; j--) {
        dp[i * w + j] = a[pre + i] === b[pre + j] ? dp[(i + 1) * w + j + 1]! + 1 : Math.max(dp[(i + 1) * w + j]!, dp[i * w + j + 1]!);
      }
    }
    let i = 0, j = 0;
    while (i < n && j < m) {
      if (a[pre + i] === b[pre + j]) { pairs.push([pre + i, pre + j]); i++; j++; }
      else if (dp[(i + 1) * w + j]! >= dp[i * w + j + 1]!) i++;
      else j++;
    }
  }
  for (let k = 0; k < suf; k++) pairs.push([a.length - suf + k, b.length - suf + k]);
  // Turn matches into opcodes.
  const ops: Array<{ op: TypedDiffOp; a0: number; a1: number; b0: number; b1: number }> = [];
  let ai = 0, bi = 0;
  const pushOp = (op: TypedDiffOp, a0: number, a1: number, b0: number, b1: number) => {
    const last = ops[ops.length - 1];
    if (last && last.op === op && last.a1 === a0 && last.b1 === b0) { last.a1 = a1; last.b1 = b1; }
    else ops.push({ op, a0, a1, b0, b1 });
  };
  for (const [pa, pb] of [...pairs, [a.length, b.length] as [number, number]]) {
    if (pa > ai && pb > bi) pushOp('replace', ai, pa, bi, pb);
    else if (pa > ai) pushOp('delete', ai, pa, bi, bi);
    else if (pb > bi) pushOp('insert', ai, ai, bi, pb);
    if (pa < a.length && pb < b.length) pushOp('equal', pa, pa + 1, pb, pb + 1);
    ai = pa + 1; bi = pb + 1;
  }
  return ops;
}

const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

/**
 * Compares a typed answer with the expected one.
 * @param expected expected answer; HTML is stripped (pass plain text if already plain)
 * @param typed what the user typed (plain text)
 */
export function compareTypedAnswer(expected: string, typed: string, opts: TypedCompareOptions = {}): TypedResult {
  const exp = expectedTextFromHtml(expected);
  const giv = normalizeTypedText(typed);
  const ua = units(giv), ub = units(exp);
  const ka = ua.map((u) => foldKey(u, opts)), kb = ub.map((u) => foldKey(u, opts));
  const ops = align(ka, kb);
  const segments: TypedSegment[] = ops.map((o) => ({ op: o.op, typed: ua.slice(o.a0, o.a1).join(''), expected: ub.slice(o.b0, o.b1).join('') }));
  const correct = ka.length === kb.length && ka.every((k, i) => k === kb[i]);

  let html: string;
  if (correct) {
    html = `<code id=typeans><span class=typeGood>${esc(giv)}</span></code>`;
  } else if (!giv) {
    html = `<code id=typeans><span class=typeMissed>${esc(exp)}</span></code>`;
  } else {
    let top = '', bottom = '';
    for (const s of segments) {
      if (s.op === 'equal') {
        top += `<span class=typeGood>${esc(s.typed)}</span>`;
        bottom += `<span class=typeGood>${esc(s.expected)}</span>`;
      } else if (s.op === 'delete') {
        top += `<span class=typeBad>${esc(s.typed)}</span>`;
      } else if (s.op === 'insert') {
        top += `<span class=typeMissed>${'-'.repeat(units(s.expected).length)}</span>`;
        bottom += `<span class=typeMissed>${esc(s.expected)}</span>`;
      } else {
        top += `<span class=typeBad>${esc(s.typed)}</span>`;
        bottom += `<span class=typeMissed>${esc(s.expected)}</span>`;
      }
    }
    html = `<code id=typeans>${top}<br><span id=typearrow>&darr;</span><br>${bottom}</code>`;
  }
  return { correct, segments, html, expected: exp, typed: giv };
}
