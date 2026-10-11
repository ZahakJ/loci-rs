/**
 * Cloze helpers for the field editor.
 *
 * The DOM selection is mapped to offsets in the field's HTML by inserting two private-use
 * sentinel characters at the selection ends and reading `innerHTML` (they survive
 * serialisation as plain characters). `applyClozeAtMarkers` is the pure step: it finds the
 * sentinels, wraps the HTML with `wrapCloze` and puts the sentinels back where the selection
 * should be restored. `clozeSpans` finds cloze syntax in plain text for in-editor highlighting.
 */
import { currentClozeNumber, nextClozeNumber, wrapCloze } from '@core/template/cloze';

export const SEL_START = '';
export const SEL_END = '';

export interface ClozeApplyResult {
  /** new HTML with SEL_START / SEL_END marking the selection to restore */
  html: string;
  /** cloze number used */
  n: number;
  /** offsets of the selection in the clean HTML (without sentinels) */
  start: number;
  end: number;
}

/** Removes both sentinels. */
export const stripMarkers = (s: string): string => s.replace(/[]/g, '');

/** Offsets of the selection (start ≤ end) in the sentinel-free HTML; null when a sentinel is missing. */
export function markerOffsets(htmlWithMarkers: string): { clean: string; start: number; end: number } | null {
  const a = htmlWithMarkers.indexOf(SEL_START);
  const b = htmlWithMarkers.indexOf(SEL_END);
  if (a < 0 || b < 0) return null;
  const clean = stripMarkers(htmlWithMarkers);
  // Each sentinel before a position shifts it by one.
  const start = a < b ? a : a - 1;
  const end = b < a ? b : b - 1;
  return { clean, start: Math.min(start, end), end: Math.max(start, end) };
}

/**
 * Wraps the selection between the sentinels in a cloze. `mode` 'next' uses the next free
 * number (Ctrl+Shift+C), 'same' reuses the highest existing one (Ctrl+Alt+Shift+C).
 * `contextHtml` (the whole note's cloze fields joined) decides the number, as Anki does
 * across fields; it defaults to this field.
 */
export function applyClozeAtMarkers(htmlWithMarkers: string, mode: 'next' | 'same', contextHtml?: string): ClozeApplyResult | null {
  const pos = markerOffsets(htmlWithMarkers);
  if (!pos) return null;
  const ctx = contextHtml ?? pos.clean;
  const n = mode === 'next' ? nextClozeNumber(ctx) : currentClozeNumber(ctx);
  const { start, end } = expandOutOfTags(pos.clean, pos.start, pos.end);
  const w = wrapCloze(pos.clean, start, end, n);
  const html = w.text.slice(0, w.selectionStart) + SEL_START + w.text.slice(w.selectionStart, w.selectionEnd) + SEL_END + w.text.slice(w.selectionEnd);
  return { html, n, start, end };
}

/** Guards against offsets that fall inside a tag (`<b|r>`): moves them to the tag's edge. */
export function expandOutOfTags(html: string, start: number, end: number): { start: number; end: number } {
  const inside = (i: number): [number, number] | null => {
    const lt = html.lastIndexOf('<', i - 1);
    const gt = html.lastIndexOf('>', i - 1);
    if (lt > gt) { const close = html.indexOf('>', lt); return [lt, close < 0 ? html.length : close + 1]; }
    return null;
  };
  const s = inside(start), e = inside(end);
  return { start: s ? s[0] : start, end: e ? e[1] : end };
}

export interface ClozeSpan { start: number; end: number; kind: 'mark' | 'body' | 'hint'; ord: number; depth: number }

/**
 * Cloze syntax in plain text, for highlighting: opening markers `{{c1::`, hint separators with
 * their hint text, closing `}}` (kind 'mark' / 'hint'), and the bodies between them (kind
 * 'body', nested bodies included with their depth). Unclosed clozes produce no spans.
 */
export function clozeSpans(text: string): ClozeSpan[] {
  const out: ClozeSpan[] = [];
  const open: Array<{ start: number; markEnd: number; ord: number; hintAt: number }> = [];
  const re = /\{\{c(\d+)(?:,\d+)*::|\}\}|::/g;
  for (const m of text.matchAll(re)) {
    const i = m.index!;
    if (m[0] === '}}') {
      const o = open.pop();
      if (!o) continue;
      const bodyEnd = o.hintAt >= 0 ? o.hintAt : i;
      out.push({ start: o.start, end: o.markEnd, kind: 'mark', ord: o.ord, depth: open.length });
      if (bodyEnd > o.markEnd) out.push({ start: o.markEnd, end: bodyEnd, kind: 'body', ord: o.ord, depth: open.length });
      if (o.hintAt >= 0) out.push({ start: o.hintAt, end: i, kind: 'hint', ord: o.ord, depth: open.length });
      out.push({ start: i, end: i + 2, kind: 'mark', ord: o.ord, depth: open.length });
    } else if (m[0] === '::') {
      const o = open[open.length - 1];
      if (o && o.hintAt < 0) o.hintAt = i;
    } else {
      open.push({ start: i, markEnd: i + m[0].length, ord: Number(m[1]), hintAt: -1 });
    }
  }
  return out.sort((a, b) => a.start - b.start || a.depth - b.depth);
}
