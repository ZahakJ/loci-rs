import { describe, expect, it } from 'vitest';
import type { SchedulingPreview } from '@core/types';
import {
  answerAudio, cappedElapsed, countOneByOne, formatClock, formatCustom, frontFieldOrd, gradeLabel, gradeShares,
  gradedAnnouncement, intervalWords, isDueSyntax, labelWords, moreNewOffer, parseCustom, sessionProgress, splitDeckName, undoSubject,
} from '@ui/views/study/format';

const preview: SchedulingPreview = {
  labels: { 1: '<10m', 2: '2d', 3: '4d', 4: '' },
  secondsUntil: { 1: 600, 2: 2 * 86400, 3: 4 * 86400, 4: 9 * 86400 },
};

describe('grade labels', () => {
  it('uses the scheduler label, else formats the seconds', () => {
    expect(gradeLabel(preview, 1)).toBe('<10m');
    expect(gradeLabel(preview, 3)).toBe('4d');
    expect(gradeLabel(preview, 4)).toBe('9d');
    expect(gradeLabel(null, 3)).toBe('');
  });
  it('announces the grade with the interval in words', () => {
    expect(gradedAnnouncement(3, preview)).toBe('Graded Good, next in 4 days');
    // the label on the button was '<10m', so that is what is said
    expect(gradedAnnouncement(1, preview)).toBe('Graded Again, next in under 10 minutes');
    expect(gradedAnnouncement(2, null)).toBe('Graded Hard');
  });
  it('announces exactly what the grade button showed', () => {
    const p: SchedulingPreview = { labels: { 1: '6m', 2: '1.2mo', 3: '3d', 4: '2y' }, secondsUntil: { 1: 400, 2: 3e6, 3: 3 * 86400, 4: 7e7 } };
    expect(gradedAnnouncement(1, p)).toBe('Graded Again, next in 6 minutes');
    expect(gradedAnnouncement(2, p)).toBe('Graded Hard, next in 1.2 months');
    expect(gradedAnnouncement(4, p)).toBe('Graded Easy, next in 2 years');
    // labels the parser does not know fall back to the seconds
    expect(gradedAnnouncement(3, { ...p, labels: { ...p.labels, 3: 'soon' } })).toBe('Graded Good, next in 3 days');
  });
  it('turns grade labels into words', () => {
    expect(labelWords('6m')).toBe('6 minutes');
    expect(labelWords('1m')).toBe('1 minute');
    expect(labelWords('<10m')).toBe('under 10 minutes');
    expect(labelWords('1h')).toBe('1 hour');
    expect(labelWords('1.5h')).toBe('1.5 hours');
    expect(labelWords('15d')).toBe('15 days');
    expect(labelWords('1.2mo')).toBe('1.2 months');
    expect(labelWords('2w')).toBe('2 weeks');
    expect(labelWords('45s')).toBe('under a minute');
    expect(labelWords('')).toBeNull();
    expect(labelWords('later')).toBeNull();
  });
  it('says intervals in words', () => {
    expect(intervalWords(20)).toBe('under a minute');
    expect(intervalWords(60)).toBe('1 minute');
    expect(intervalWords(3 * 3600)).toBe('3 hours');
    expect(intervalWords(86400)).toBe('1 day');
    expect(intervalWords(75 * 86400)).toBe('2.5 months');
    expect(intervalWords(800 * 86400)).toBe('2.2 years');
  });
});

describe('session helpers', () => {
  it('strips the undo verb', () => {
    expect(undoSubject('Undo Good')).toBe('Good');
    expect(undoSubject('Bury')).toBe('Bury');
  });
  it('computes progress and caps answer time', () => {
    expect(sessionProgress(0, 0)).toBe(0);
    expect(sessionProgress(3, 1)).toBe(0.75);
    expect(cappedElapsed(90_000, 60)).toBe(60_000);
    expect(cappedElapsed(4_200.4, 60)).toBe(4_200);
    expect(cappedElapsed(90_000, 0)).toBe(90_000);
    expect(formatClock(65_400)).toBe('1:05');
  });
  it('shares grades for the summary bar', () => {
    expect(gradeShares({ again: 1, hard: 1, good: 2, easy: 0 })).toEqual({ again: 0.25, hard: 0.25, good: 0.5, easy: 0 });
    expect(gradeShares({ again: 0, hard: 0, good: 0, easy: 0 })).toEqual({ again: 0, hard: 0, good: 0, easy: 0 });
  });
  it('splits deck names', () => {
    expect(splitDeckName('Languages::Arabic')).toEqual({ parents: ['Languages'], leaf: 'Arabic' });
    expect(splitDeckName('Default')).toEqual({ parents: [], leaf: 'Default' });
  });
  it('counts one-by-one clozes', () => {
    expect(countOneByOne('<span class="cloze" data-cloze-index="0">a</span> <span class="cloze" data-cloze-index="1">b</span>')).toBe(2);
    expect(countOneByOne('<span class="cloze">a</span>')).toBe(0);
  });
  it('skips the question audio on the answer unless asked', () => {
    expect(answerAudio(['q.mp3'], ['q.mp3', 'a.mp3'], false)).toEqual(['a.mp3']);
    expect(answerAudio(['q.mp3'], ['q.mp3', 'a.mp3'], true)).toEqual(['q.mp3', 'a.mp3']);
    expect(answerAudio([], ['a.mp3'], false)).toEqual(['a.mp3']);
  });
});

describe('set due date syntax', () => {
  it("accepts Anki's forms", () => {
    for (const ok of ['0', '1', '1!', '3-7', '3-7!', ' 14 ']) expect(isDueSyntax(ok), ok).toBe(true);
  });
  it('rejects the rest', () => {
    for (const bad of ['', '-1', '7-3', 'a', '1!!', '1-', '1.5', '3 - 7']) expect(isDueSyntax(bad), bad).toBe(false);
  });
});

describe('more new cards offer', () => {
  it("offers a day's allowance, capped by what is left", () => {
    expect(moreNewOffer(94, 20)).toBe(20);
    expect(moreNewOffer(7, 20)).toBe(7);
    expect(moreNewOffer(50, 0)).toBe(10);
    expect(moreNewOffer(0, 20)).toBe(0);
  });
});

describe('custom session param', () => {
  it('parses each kind', () => {
    expect(parseCustom('ahead:3')).toEqual({ kind: 'ahead', days: 3 });
    expect(parseCustom('forgotten:7')).toEqual({ kind: 'forgotten', days: 7 });
    expect(parseCustom('preview')).toEqual({ kind: 'preview' });
    expect(parseCustom('search:tag:verb -is:new')).toEqual({ kind: 'search', query: 'tag:verb -is:new', limit: 100 });
    expect(parseCustom('search:deck:Spanish|50')).toEqual({ kind: 'search', query: 'deck:Spanish', limit: 50 });
  });
  it('rejects nonsense', () => {
    for (const bad of ['', 'ahead', 'ahead:0', 'ahead:x', 'search:', 'later:3']) expect(parseCustom(bad)).toBeNull();
  });
  it('round-trips', () => {
    for (const s of ['ahead:3', 'forgotten:30', 'preview', 'search:tag:x', 'search:tag:x|50']) expect(formatCustom(parseCustom(s)!)).toBe(s);
  });
});

describe('frontFieldOrd', () => {
  it('finds the field the question shows', () => {
    const t = [{ ord: 0, qfmt: '{{Front}}' }, { ord: 1, qfmt: '{{#Back}}{{Back}}{{/Back}}' }];
    expect(frontFieldOrd(t, false, 0, ['Front', 'Back'], ['a', 'b'])).toBe(0);
    expect(frontFieldOrd(t, false, 1, ['Front', 'Back'], ['a', 'b'])).toBe(1);
    expect(frontFieldOrd([{ ord: 0, qfmt: '{{hint:Extra}} {{type:Back}}' }], false, 0, ['Front', 'Back', 'Extra'], [])).toBe(2);
  });
  it('uses the field holding this cloze', () => {
    expect(frontFieldOrd([], true, 1, ['Text', 'Extra'], ['{{c1::a}}', '{{c2::b}}'])).toBe(1);
  });
});
