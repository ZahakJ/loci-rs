/**
 * Cloze highlighting in editable fields without touching their DOM: the CSS Custom Highlight
 * API paints ranges (`::highlight(loci-cloze)` etc.), so the saved HTML never changes and the
 * caret is never disturbed. Browsers without the API simply show no highlight.
 */
import { clozeSpans } from './cloze';

type Kind = 'loci-cloze-mark' | 'loci-cloze-body' | 'loci-cloze-hint';
const registry = new Map<HTMLElement, Record<Kind, Range[]>>();

interface HighlightLike { add(r: Range): void }
type HighlightCtor = new (...ranges: Range[]) => HighlightLike;
const supported = (): boolean => typeof CSS !== 'undefined' && 'highlights' in CSS && typeof (globalThis as { Highlight?: unknown }).Highlight === 'function';

function rebuild() {
  if (!supported()) return;
  const H = (globalThis as unknown as { Highlight: HighlightCtor }).Highlight;
  const reg = (CSS as unknown as { highlights: Map<string, HighlightLike> }).highlights;
  for (const kind of ['loci-cloze-mark', 'loci-cloze-body', 'loci-cloze-hint'] as Kind[]) {
    const all: Range[] = [];
    for (const r of registry.values()) all.push(...r[kind]);
    if (all.length) reg.set(kind, new H(...all)); else reg.delete(kind);
  }
}

/** Recomputes the cloze ranges of one editor root. */
export function highlightClozes(root: HTMLElement): void {
  if (!supported()) return;
  const nodes: Text[] = [];
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  let text = '';
  const starts: number[] = [];
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    nodes.push(n as Text);
    starts.push(text.length);
    text += (n as Text).data;
  }
  const at = (offset: number): [Text, number] | null => {
    if (!nodes.length) return null;
    let lo = 0, hi = nodes.length - 1;
    while (lo < hi) { const mid = (lo + hi + 1) >> 1; if (starts[mid]! <= offset) lo = mid; else hi = mid - 1; }
    return [nodes[lo]!, Math.min(offset - starts[lo]!, nodes[lo]!.data.length)];
  };
  const out: Record<Kind, Range[]> = { 'loci-cloze-mark': [], 'loci-cloze-body': [], 'loci-cloze-hint': [] };
  if (text.includes('{{c')) {
    for (const s of clozeSpans(text)) {
      if (s.kind === 'body' && s.depth > 0) continue; // nested bodies already sit inside the outer one
      const a = at(s.start), b = at(s.end);
      if (!a || !b) continue;
      const r = document.createRange();
      try { r.setStart(a[0], a[1]); r.setEnd(b[0], b[1]); } catch { continue; }
      out[s.kind === 'mark' ? 'loci-cloze-mark' : s.kind === 'hint' ? 'loci-cloze-hint' : 'loci-cloze-body'].push(r);
    }
  }
  registry.set(root, out);
  rebuild();
}

export function releaseHighlights(root: HTMLElement): void {
  if (registry.delete(root)) rebuild();
}
