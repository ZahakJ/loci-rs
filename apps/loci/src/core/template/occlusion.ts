/**
 * Image Occlusion in Anki's built-in format (research/anki-format.md §5.4).
 *
 * The `Occlusion` field holds one cloze per shape, each followed by `<br>`:
 *   {{c1::image-occlusion:rect:left=.2325:top=.3261:width=.202:height=.0975:oi=1}}<br>
 *   {{c2::image-occlusion:ellipse:left=.1:top=.2:rx=.05:ry=.1}}<br>
 *   {{c3::image-occlusion:polygon:points=.1,.1 .2,.2 .3,.1}}<br>
 *   {{c0::image-occlusion:text:text=Label\:x:left=.4:top=.5:scale=1:fs=.05}}<br>
 * Values escape `\` and `:` as `\\` and `\:`. Coordinates are normalised to 0..1 of the image
 * (current Anki); very old notes may hold pixels (see `occlusionUnits`). Ellipses are given by
 * their bounding box's top-left plus radii; we expose width = 2·rx, height = 2·ry.
 * `oi=1` (occlude inactive) means "hide all, guess one"; otherwise "hide one, guess one".
 */
import type { OcclusionPayload, OcclusionShape } from '../types';
import { decodeEntities } from '../text';
import { imageOcclusionText, parseClozes, type ClozeTextOrCloze } from './cloze';

/** A parsed shape plus what is needed to write it back unchanged. */
export interface IoShape extends OcclusionShape {
  /** `oi=1`: all shapes are masked while this one is asked ("hide all, guess one"). */
  occludeInactive: boolean;
  /** every key=value pair in source order (unescaped), including ones Loci does not interpret (angle, fill, scale, fs…) */
  props: Array<[string, string]>;
}

const SHAPES = new Set(['rect', 'ellipse', 'polygon', 'text']);

/** Splits on unescaped ':' and unescapes `\:` and `\\`. */
function splitEscaped(s: string): string[] {
  const out: string[] = [];
  let cur = '';
  for (let i = 0; i < s.length; i++) {
    const ch = s[i]!;
    if (ch === '\\' && i + 1 < s.length) { cur += s[i + 1]; i++; continue; }
    if (ch === ':') { out.push(cur); cur = ''; continue; }
    cur += ch;
  }
  out.push(cur);
  return out;
}
const escapeValue = (v: string) => v.replace(/\\/g, '\\\\').replace(/:/g, '\\:');

const num = (v: string | undefined): number | undefined => {
  if (v === undefined || v.trim() === '') return undefined;
  const n = Number(v);
  return Number.isFinite(n) ? n : undefined;
};

/** Parses the body after `image-occlusion:` into a shape (null if the shape kind is unknown). */
export function parseOcclusionBody(body: string, ord: number): IoShape | null {
  const [kind = '', ...pairs] = splitEscaped(body);
  if (!SHAPES.has(kind)) return null;
  const props: Array<[string, string]> = [];
  for (const p of pairs) {
    const eq = p.indexOf('=');
    if (eq <= 0) continue;
    props.push([p.slice(0, eq), p.slice(eq + 1)]);
  }
  const get = (k: string) => props.find(([key]) => key === k)?.[1];
  const shape: IoShape = { kind: kind as IoShape['kind'], ord, occludeInactive: get('oi') === '1', props };
  const left = num(get('left')), top = num(get('top'));
  if (left !== undefined) shape.left = left;
  if (top !== undefined) shape.top = top;
  let width = num(get('width')), height = num(get('height'));
  if (kind === 'ellipse') {
    const rx = num(get('rx')), ry = num(get('ry'));
    if (width === undefined && rx !== undefined) width = rx * 2;
    if (height === undefined && ry !== undefined) height = ry * 2;
  }
  if (width !== undefined) shape.width = width;
  if (height !== undefined) shape.height = height;
  if (kind === 'polygon') {
    const pts = (get('points') ?? '').trim().split(/\s+/).filter(Boolean).map((xy) => {
      const [x, y] = xy.split(',');
      return [Number(x), Number(y)] as [number, number];
    }).filter(([x, y]) => Number.isFinite(x) && Number.isFinite(y));
    shape.points = pts;
    if (pts.length && shape.left === undefined) {
      const xs = pts.map((p) => p[0]), ys = pts.map((p) => p[1]);
      shape.left = Math.min(...xs); shape.top = Math.min(...ys);
      shape.width = Math.max(...xs) - shape.left; shape.height = Math.max(...ys) - shape.top;
    }
  }
  if (kind === 'text') shape.text = get('text') ?? '';
  return shape;
}

/** All IO shapes in an Occlusion field, in source order (grouped shapes share an ordinal). */
export function parseOcclusionField(field: string): IoShape[] {
  const out: IoShape[] = [];
  const walk = (nodes: ClozeTextOrCloze[]) => {
    for (const n of nodes) {
      if (n.kind !== 'cloze') continue;
      const body = imageOcclusionText(n);
      if (body !== null) {
        const s = parseOcclusionBody(decodeEntities(body), n.ordinals[0] ?? 0);
        if (s) out.push(s);
      } else walk(n.nodes);
    }
  };
  walk(parseClozes(field));
  return out;
}

/** Anki's number display for IO coordinates: up to 4 decimals, no leading/trailing zeros (`.2325`). */
export function formatIoNumber(n: number): string {
  if (!Number.isFinite(n) || n === 0) return '0';
  let s = n.toFixed(4);
  if (s.includes('.')) s = s.replace(/0+$/, '').replace(/\.$/, '');
  return s.replace(/^(-?)0\./, '$1.');
}

type ShapeInput = OcclusionShape & { occludeInactive?: boolean; props?: Array<[string, string]> };

/** Geometry props for a shape, derived from its typed fields (used when writing). */
function geometryProps(s: ShapeInput): Array<[string, string]> {
  const f = (n: number | undefined) => formatIoNumber(n ?? 0);
  switch (s.kind) {
    case 'rect': return [['left', f(s.left)], ['top', f(s.top)], ['width', f(s.width)], ['height', f(s.height)]];
    case 'ellipse': return [['left', f(s.left)], ['top', f(s.top)], ['rx', f((s.width ?? 0) / 2)], ['ry', f((s.height ?? 0) / 2)]];
    case 'polygon': return [['points', (s.points ?? []).map(([x, y]) => `${formatIoNumber(x)},${formatIoNumber(y)}`).join(' ')]];
    case 'text': return [['text', s.text ?? ''], ['left', f(s.left)], ['top', f(s.top)]];
  }
}

/** Serialises one shape as Anki writes it: `{{cN::image-occlusion:kind:k=v…}}`. */
export function serializeOcclusionShape(s: ShapeInput): string {
  const geo = geometryProps(s);
  const geoKeys = new Set(geo.map(([k]) => k));
  // Keep the original key order and extra keys; overwrite geometry with current values.
  const pairs: Array<[string, string]> = [];
  const seen = new Set<string>();
  const parsed = parseOcclusionBody(`${s.kind}:${(s.props ?? []).map(([k, v]) => `${k}=${escapeValue(v)}`).join(':')}`, s.ord);
  const unchanged = parsed !== null && sameGeometry(parsed, s);
  for (const [k, v] of s.props ?? []) {
    if (k === 'oi') continue;
    if (geoKeys.has(k)) {
      pairs.push([k, unchanged ? v : geo.find(([g]) => g === k)![1]]);
    } else if (!(s.kind === 'ellipse' && (k === 'width' || k === 'height'))) pairs.push([k, v]);
    seen.add(k);
  }
  for (const [k, v] of geo) if (!seen.has(k)) pairs.push([k, v]);
  if (s.occludeInactive && s.kind !== 'text') pairs.push(['oi', '1']);
  const body = [s.kind, ...pairs.map(([k, v]) => `${k}=${escapeValue(v)}`)].join(':');
  return `{{c${s.ord}::image-occlusion:${body}}}`;
}

function sameGeometry(a: OcclusionShape, b: OcclusionShape): boolean {
  const eq = (x?: number, y?: number) => (x === undefined && y === undefined) || (x !== undefined && y !== undefined && Math.abs(x - y) < 5e-5);
  if (a.kind === 'polygon') return JSON.stringify(a.points ?? []) === JSON.stringify(b.points ?? []);
  return eq(a.left, b.left) && eq(a.top, b.top) && eq(a.width, b.width) && eq(a.height, b.height) && (a.text ?? '') === (b.text ?? '');
}

/** Writes an Occlusion field: one cloze per shape, each followed by `<br>` (Anki's layout). */
export function serializeOcclusionField(shapes: ShapeInput[]): string {
  return shapes.map((s) => `${serializeOcclusionShape(s)}<br>`).join('');
}

/** 'pixels' when any coordinate exceeds 1 (legacy notes); the UI then scales by the image's natural size. */
export function occlusionUnits(shapes: OcclusionShape[]): 'normalized' | 'pixels' {
  const big = (n?: number) => n !== undefined && n > 1.0001;
  return shapes.some((s) => big(s.left) || big(s.top) || big(s.width) || big(s.height) || (s.points ?? []).some(([x, y]) => big(x) || big(y))) ? 'pixels' : 'normalized';
}

/** Mode for a card: 'hide-all' when the active shapes carry `oi=1`, else 'hide-one'. */
export function occlusionMode(shapes: IoShape[], activeOrd: number, fallback: 'hide-all' | 'hide-one' = 'hide-one'): 'hide-all' | 'hide-one' {
  const active = shapes.filter((s) => s.ord === activeOrd && s.kind !== 'text');
  if (!active.length) return fallback;
  return active.some((s) => s.occludeInactive) ? 'hide-all' : 'hide-one';
}

/** First `<img src>` filename in a field (entities decoded), or ''. */
export function firstImageName(html: string): string {
  const m = /<img\b[^>]*?\bsrc\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))/i.exec(html);
  return m ? decodeEntities(m[1] ?? m[2] ?? m[3] ?? '') : '';
}

/**
 * Payload for one card. `cardOrd` is 0-based (activeOrd = cardOrd + 1). Returns undefined when
 * the field holds no shapes. `image` is the filename as stored; callers resolve it to a URL.
 */
export function occlusionPayload(occlusionField: string, imageField: string, cardOrd: number, fallback: 'hide-all' | 'hide-one' = 'hide-one'): OcclusionPayload | undefined {
  const shapes = parseOcclusionField(occlusionField);
  if (!shapes.length) return undefined;
  const activeOrd = cardOrd + 1;
  return {
    image: firstImageName(imageField),
    shapes: shapes.map(({ props: _p, occludeInactive: _o, ...s }) => s),
    mode: occlusionMode(shapes, activeOrd, fallback),
    activeOrd,
  };
}
