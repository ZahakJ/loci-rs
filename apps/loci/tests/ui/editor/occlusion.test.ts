import { describe, expect, it } from 'vitest';
import { hitTest, initialOcclusionState, maxOrd, occlusionReducer as R, serializeShapes } from '@ui/views/edit/occlusion/model';

const FIELD = '{{c1::image-occlusion:rect:left=.1:top=.2:width=.3:height=.4:oi=1}}<br>{{c2::image-occlusion:ellipse:left=.5:top=.5:rx=.1:ry=.05:oi=1}}<br>';

describe('occlusion editor reducer', () => {
  it('loads Anki shapes and infers the mode', () => {
    const s = initialOcclusionState(FIELD);
    expect(s.shapes.map((x) => [x.kind, x.ord])).toEqual([['rect', 1], ['ellipse', 2]]);
    expect(s.shapes[1]!.width).toBeCloseTo(0.2);
    expect(s.mode).toBe('hide-all');
    expect(initialOcclusionState(FIELD.replace(/:oi=1/g, '')).mode).toBe('hide-one');
  });
  it('round-trips unchanged shapes byte for byte', () => {
    expect(serializeShapes(initialOcclusionState(FIELD))).toBe(FIELD);
  });
  it('adds shapes with the next ordinal and selects them; ignores tiny ones', () => {
    let s = initialOcclusionState(FIELD);
    s = R(s, { type: 'add', shape: { kind: 'rect', left: 0.6, top: 0.1, width: 0.2, height: 0.1 } });
    expect(s.shapes.at(-1)!.ord).toBe(3);
    expect(s.selected).toEqual([s.shapes.at(-1)!.id]);
    const before = s;
    s = R(s, { type: 'add', shape: { kind: 'rect', left: 0.6, top: 0.1, width: 0.001, height: 0.1 } });
    expect(s).toBe(before);
    s = R(s, { type: 'add', shape: { kind: 'text', left: 0.1, top: 0.9, width: 0.1, height: 0.04, text: 'Aorta' } });
    expect(s.shapes.at(-1)!.ord).toBe(0);
  });
  it('moves within the image bounds, polygons included', () => {
    let s = initialOcclusionState(FIELD);
    const id = s.shapes[0]!.id;
    s = R(s, { type: 'move', ids: [id], dx: 0.05, dy: -0.5 });
    expect(s.shapes[0]).toMatchObject({ left: 0.15, top: 0 });
    s = R(s, { type: 'move', ids: [id], dx: 2, dy: 0 });
    expect(s.shapes[0]!.left + s.shapes[0]!.width).toBeCloseTo(1);
    s = R(s, { type: 'add', shape: { kind: 'polygon', left: 0, top: 0, width: 0, height: 0, points: [[0.1, 0.1], [0.3, 0.1], [0.2, 0.3]] } });
    const poly = s.shapes.at(-1)!;
    expect(poly).toMatchObject({ left: 0.1, top: 0.1, width: 0.2, height: 0.2 });
    s = R(s, { type: 'move', ids: [poly.id], dx: 0.1, dy: 0.1 });
    expect(s.shapes.at(-1)!.points).toEqual([[0.2, 0.2], [0.4, 0.2], [0.3, 0.4]]);
  });
  it('resizes by handle with a minimum size, scaling polygon points', () => {
    let s = initialOcclusionState(FIELD);
    const id = s.shapes[0]!.id;
    s = R(s, { type: 'resize', id, handle: 'se', x: 0.6, y: 0.8 });
    expect(s.shapes[0]).toMatchObject({ left: 0.1, top: 0.2, width: 0.5, height: 0.6 });
    s = R(s, { type: 'resize', id, handle: 'w', x: 0.9, y: 0 });
    expect(s.shapes[0]!.width).toBeCloseTo(0.01);
    s = R(s, { type: 'add', shape: { kind: 'polygon', left: 0, top: 0, width: 0, height: 0, points: [[0.1, 0.1], [0.3, 0.1], [0.3, 0.3]] } });
    const pid = s.shapes.at(-1)!.id;
    s = R(s, { type: 'resize', id: pid, handle: 'e', x: 0.5, y: 0 });
    expect(s.shapes.at(-1)!.points).toEqual([[0.1, 0.1], [0.5, 0.1], [0.5, 0.3]]);
  });
  it('deletes, groups and ungroups', () => {
    let s = initialOcclusionState(FIELD);
    const [a, b] = s.shapes.map((x) => x.id) as [string, string];
    s = R(s, { type: 'group', ids: [a, b] });
    expect(s.shapes.map((x) => x.ord)).toEqual([1, 1]);
    s = R(s, { type: 'ungroup', ids: [a, b] });
    expect(s.shapes.map((x) => x.ord)).toEqual([1, 2]);
    s = R(s, { type: 'select', ids: [a] });
    s = R(s, { type: 'delete', ids: [a] });
    expect(s.shapes.map((x) => x.id)).toEqual([b]);
    expect(s.selected).toEqual([]);
    expect(maxOrd(s.shapes)).toBe(2);
  });
  it('serialises the mode as oi=1 and keeps labels', () => {
    let s = initialOcclusionState('');
    s = R(s, { type: 'add', shape: { kind: 'rect', left: 0.25, top: 0.5, width: 0.125, height: 0.25 } });
    s = R(s, { type: 'mode', mode: 'hide-one' });
    expect(serializeShapes(s)).toBe('{{c1::image-occlusion:rect:left=.25:top=.5:width=.125:height=.25}}<br>');
    s = R(s, { type: 'mode', mode: 'hide-all' });
    expect(serializeShapes(s)).toContain(':oi=1}}');
    s = R(s, { type: 'add', shape: { kind: 'text', left: 0.1, top: 0.1, width: 0.1, height: 0.05, text: 'a:b' } });
    expect(serializeShapes(s)).toContain('{{c0::image-occlusion:text:text=a\\:b:left=.1:top=.1}}');
  });
  it('hit-tests topmost first', () => {
    const s = initialOcclusionState(FIELD);
    expect(hitTest(s.shapes, 0.2, 0.3)?.ord).toBe(1);
    expect(hitTest(s.shapes, 0.95, 0.95)).toBeNull();
  });
});
