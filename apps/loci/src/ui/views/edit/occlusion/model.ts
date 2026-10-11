/**
 * Image-occlusion editor state (pure reducer; tested). Coordinates are normalised 0..1 of the
 * image, as Anki stores them. Shapes keep their original `props` so unknown keys survive a
 * round trip through `serializeOcclusionField`.
 */
import { parseOcclusionField, serializeOcclusionField, type IoShape } from '@core/template/occlusion';

export type ShapeKind = 'rect' | 'ellipse' | 'polygon' | 'text';
export type Tool = 'select' | ShapeKind;
export type Handle = 'nw' | 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w';

export interface EditShape {
  id: string;
  kind: ShapeKind;
  /** cloze ordinal (card number); text labels use 0 and never make a card */
  ord: number;
  left: number; top: number; width: number; height: number;
  points?: Array<[number, number]>;
  text?: string;
  props?: Array<[string, string]>;
}

export interface OcclusionState {
  shapes: EditShape[];
  selected: string[];
  tool: Tool;
  mode: 'hide-all' | 'hide-one';
  seq: number;
}

export type OcclusionAction =
  | { type: 'load'; field: string; mode?: 'hide-all' | 'hide-one' }
  | { type: 'tool'; tool: Tool }
  | { type: 'mode'; mode: 'hide-all' | 'hide-one' }
  | { type: 'add'; shape: Omit<EditShape, 'id' | 'ord'> & { ord?: number }; select?: boolean }
  | { type: 'select'; ids: string[]; additive?: boolean }
  | { type: 'selectAll' }
  | { type: 'move'; ids: string[]; dx: number; dy: number }
  | { type: 'resize'; id: string; handle: Handle; x: number; y: number }
  | { type: 'delete'; ids: string[] }
  | { type: 'label'; id: string; text: string }
  | { type: 'group'; ids: string[] }
  | { type: 'ungroup'; ids: string[] }
  | { type: 'clear' }
  /** legacy notes stored pixels: convert to 0..1 of the image's natural size */
  | { type: 'normalize'; width: number; height: number };

export const MIN_SIZE = 0.01;
const clamp01 = (v: number) => Math.min(1, Math.max(0, v));
const round = (v: number) => Math.round(v * 10000) / 10000;

export function initialOcclusionState(field = '', mode?: 'hide-all' | 'hide-one'): OcclusionState {
  return occlusionReducer({ shapes: [], selected: [], tool: 'rect', mode: mode ?? 'hide-all', seq: 0 }, { type: 'load', field, mode });
}

/** Highest card ordinal in use (0 when none). */
export function maxOrd(shapes: EditShape[]): number {
  return shapes.reduce((m, s) => Math.max(m, s.ord), 0);
}

function fromIo(s: IoShape, id: string): EditShape {
  const shape: EditShape = {
    id, kind: s.kind, ord: s.ord,
    left: s.left ?? 0, top: s.top ?? 0, width: s.width ?? 0, height: s.height ?? 0,
    props: s.props,
  };
  if (s.points) shape.points = s.points.map(([x, y]) => [x, y]);
  if (s.text !== undefined) shape.text = s.text;
  return shape;
}

function bbox(points: Array<[number, number]>): { left: number; top: number; width: number; height: number } {
  const xs = points.map((p) => p[0]), ys = points.map((p) => p[1]);
  const left = Math.min(...xs), top = Math.min(...ys);
  return { left, top, width: Math.max(...xs) - left, height: Math.max(...ys) - top };
}

/** Moves a shape by (dx, dy), clamped so it stays inside the image. */
function translate(s: EditShape, dx: number, dy: number): EditShape {
  const cdx = Math.min(Math.max(dx, -s.left), 1 - s.left - s.width);
  const cdy = Math.min(Math.max(dy, -s.top), 1 - s.top - s.height);
  const next: EditShape = { ...s, left: round(s.left + cdx), top: round(s.top + cdy) };
  if (s.points) next.points = s.points.map(([x, y]) => [round(x + cdx), round(y + cdy)]);
  return next;
}

/** Drags one handle of the bounding box to (x, y); polygons scale their points. */
function resize(s: EditShape, handle: Handle, x: number, y: number): EditShape {
  let l = s.left, t = s.top, r = s.left + s.width, b = s.top + s.height;
  const px = clamp01(x), py = clamp01(y);
  if (handle.includes('w')) l = Math.min(px, r - MIN_SIZE);
  if (handle.includes('e')) r = Math.max(px, l + MIN_SIZE);
  if (handle.includes('n')) t = Math.min(py, b - MIN_SIZE);
  if (handle.includes('s')) b = Math.max(py, t + MIN_SIZE);
  const next: EditShape = { ...s, left: round(l), top: round(t), width: round(r - l), height: round(b - t) };
  if (s.points && s.width > 0 && s.height > 0) {
    const sx = (r - l) / s.width, sy = (b - t) / s.height;
    next.points = s.points.map(([px0, py0]) => [round(l + (px0 - s.left) * sx), round(t + (py0 - s.top) * sy)]);
  }
  return next;
}

export function occlusionReducer(state: OcclusionState, action: OcclusionAction): OcclusionState {
  switch (action.type) {
    case 'load': {
      const parsed = parseOcclusionField(action.field);
      let seq = state.seq;
      const shapes = parsed.map((s) => fromIo(s, `s${++seq}`));
      const mode = action.mode ?? (parsed.some((s) => s.kind !== 'text' && !s.occludeInactive) ? 'hide-one' : parsed.length ? 'hide-all' : state.mode);
      return { ...state, shapes, selected: [], seq, mode };
    }
    case 'tool': return { ...state, tool: action.tool, selected: action.tool === 'select' ? state.selected : [] };
    case 'mode': return { ...state, mode: action.mode };
    case 'add': {
      const id = `s${state.seq + 1}`;
      const sh = action.shape;
      let geo = { left: sh.left, top: sh.top, width: sh.width, height: sh.height };
      if (sh.points && sh.points.length) geo = bbox(sh.points);
      if (sh.kind !== 'text' && (geo.width < MIN_SIZE || geo.height < MIN_SIZE)) return state;
      const ord = sh.ord ?? (sh.kind === 'text' ? 0 : maxOrd(state.shapes) + 1);
      const shape: EditShape = { ...sh, ...geo, left: round(geo.left), top: round(geo.top), width: round(geo.width), height: round(geo.height), id, ord };
      return { ...state, seq: state.seq + 1, shapes: [...state.shapes, shape], selected: action.select === false ? state.selected : [id] };
    }
    case 'select': {
      if (!action.additive) return { ...state, selected: [...new Set(action.ids)] };
      const set = new Set(state.selected);
      for (const id of action.ids) { if (set.has(id)) set.delete(id); else set.add(id); }
      return { ...state, selected: [...set] };
    }
    case 'selectAll': return { ...state, selected: state.shapes.map((s) => s.id) };
    case 'move': {
      if (!action.ids.length || (action.dx === 0 && action.dy === 0)) return state;
      const ids = new Set(action.ids);
      return { ...state, shapes: state.shapes.map((s) => (ids.has(s.id) ? translate(s, action.dx, action.dy) : s)) };
    }
    case 'resize':
      return { ...state, shapes: state.shapes.map((s) => (s.id === action.id ? resize(s, action.handle, action.x, action.y) : s)) };
    case 'delete': {
      const ids = new Set(action.ids);
      return { ...state, shapes: state.shapes.filter((s) => !ids.has(s.id)), selected: state.selected.filter((id) => !ids.has(id)) };
    }
    case 'label':
      return { ...state, shapes: state.shapes.map((s) => (s.id === action.id ? { ...s, text: action.text } : s)) };
    case 'group': {
      const targets = state.shapes.filter((s) => action.ids.includes(s.id) && s.kind !== 'text');
      if (targets.length < 2) return state;
      const ord = Math.min(...targets.map((s) => s.ord));
      return { ...state, shapes: state.shapes.map((s) => (targets.includes(s) ? { ...s, ord } : s)) };
    }
    case 'ungroup': {
      let next = maxOrd(state.shapes);
      const seen = new Set<number>();
      return {
        ...state,
        shapes: state.shapes.map((s) => {
          if (!action.ids.includes(s.id) || s.kind === 'text') return s;
          if (!seen.has(s.ord)) { seen.add(s.ord); return s; }
          return { ...s, ord: ++next };
        }),
      };
    }
    case 'clear': return { ...state, shapes: [], selected: [] };
    case 'normalize': {
      if (!(action.width > 0 && action.height > 0)) return state;
      const fx = (v: number) => round(v / action.width), fy = (v: number) => round(v / action.height);
      return {
        ...state,
        shapes: state.shapes.map((sh) => {
          const next: EditShape = { ...sh, left: fx(sh.left), top: fy(sh.top), width: fx(sh.width), height: fy(sh.height), props: sh.props?.filter(([k]) => !['left', 'top', 'width', 'height', 'rx', 'ry', 'points'].includes(k)) };
          if (sh.points) next.points = sh.points.map(([x, y]) => [fx(x), fy(y)]);
          return next;
        }),
      };
    }
  }
}

/** Writes the Occlusion field (Anki format). `hide-all` marks each shape `oi=1`. */
export function serializeShapes(state: Pick<OcclusionState, 'shapes' | 'mode'>): string {
  return serializeOcclusionField(state.shapes.map((s) => {
    const out: Parameters<typeof serializeOcclusionField>[0][number] = {
      kind: s.kind, ord: s.ord, left: s.left, top: s.top, width: s.width, height: s.height,
      occludeInactive: state.mode === 'hide-all',
      props: s.props?.filter(([k]) => k !== 'oi'),
    };
    if (s.points) out.points = s.points;
    if (s.text !== undefined) out.text = s.text;
    return out;
  }));
}

/** Shape under a point (topmost first), with a small tolerance for thin shapes. */
export function hitTest(shapes: EditShape[], x: number, y: number, tol = 0.006): EditShape | null {
  for (let i = shapes.length - 1; i >= 0; i--) {
    const s = shapes[i]!;
    if (x >= s.left - tol && x <= s.left + s.width + tol && y >= s.top - tol && y <= s.top + s.height + tol) return s;
  }
  return null;
}
