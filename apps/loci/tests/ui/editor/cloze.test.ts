import { describe, expect, it } from 'vitest';
import { SEL_END as E, SEL_START as S, applyClozeAtMarkers, clozeSpans, expandOutOfTags, markerOffsets, stripMarkers } from '@ui/editor/cloze';

describe('selection markers → HTML offsets', () => {
  it('finds offsets regardless of marker order', () => {
    expect(markerOffsets(`ab${S}cd${E}ef`)).toEqual({ clean: 'abcdef', start: 2, end: 4 });
    expect(markerOffsets(`ab${E}cd${S}ef`)).toEqual({ clean: 'abcdef', start: 2, end: 4 });
    expect(markerOffsets(`ab${S}${E}cd`)).toEqual({ clean: 'abcd', start: 2, end: 2 });
    expect(markerOffsets('abc')).toBeNull();
  });
  it('maps through markup and entities', () => {
    const r = markerOffsets(`<b>Paris</b> is the ${S}capital &amp; seat${E} of France`)!;
    expect(r.clean.slice(r.start, r.end)).toBe('capital &amp; seat');
  });
});

describe('applyClozeAtMarkers', () => {
  it('wraps with the next number and keeps the selection on the content', () => {
    const r = applyClozeAtMarkers(`{{c1::Paris}} is the ${S}capital${E} of France`, 'next')!;
    expect(r.n).toBe(2);
    expect(stripMarkers(r.html)).toBe('{{c1::Paris}} is the {{c2::capital}} of France');
    expect(r.html).toContain(`{{c2::${S}capital${E}}}`);
  });
  it('reuses the current number for "same"', () => {
    const r = applyClozeAtMarkers(`{{c1::a}} {{c3::b}} ${S}c${E}`, 'same')!;
    expect(stripMarkers(r.html)).toBe('{{c1::a}} {{c3::b}} {{c3::c}}');
  });
  it('starts at 1 and places the caret inside an empty cloze', () => {
    const r = applyClozeAtMarkers(`x ${S}${E}y`, 'next')!;
    expect(r.html).toBe(`x {{c1::${S}${E}}}y`);
  });
  it('uses the whole note as context for the number', () => {
    const r = applyClozeAtMarkers(`${S}z${E}`, 'next', '{{c4::other field}}')!;
    expect(stripMarkers(r.html)).toBe('{{c5::z}}');
  });
  it('wraps across inline tags without breaking them', () => {
    const r = applyClozeAtMarkers(`<b>bo${S}ld</b> te${E}xt`, 'next')!;
    expect(stripMarkers(r.html)).toBe('<b>bo{{c1::ld</b> te}}xt');
  });
  it('never splits a tag', () => {
    expect(expandOutOfTags('a<br>b', 3, 3)).toEqual({ start: 1, end: 5 });
    expect(expandOutOfTags('ab', 1, 2)).toEqual({ start: 1, end: 2 });
  });
});

describe('clozeSpans', () => {
  it('marks openers, bodies, hints and closers', () => {
    const text = 'The {{c1::heart::organ}} pumps';
    const spans = clozeSpans(text).map((s) => [s.kind, text.slice(s.start, s.end)]);
    expect(spans).toEqual([['mark', '{{c1::'], ['body', 'heart'], ['hint', '::organ'], ['mark', '}}']]);
  });
  it('handles nesting and ignores unclosed clozes', () => {
    const text = '{{c1::a {{c2::b}} c}} {{c3::open';
    const spans = clozeSpans(text);
    expect(spans.filter((s) => s.kind === 'body').map((s) => [s.ord, text.slice(s.start, s.end)])).toEqual([[1, 'a {{c2::b}} c'], [2, 'b']]);
    expect(spans.some((s) => s.ord === 3)).toBe(false);
  });
});
