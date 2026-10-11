/**
 * Image-occlusion overlay (DESIGN §8): SVG markup drawn over the image from an OcclusionPayload.
 * Pure (tested). Coordinates are normalised 0..1 unless the note is a legacy pixel note, in which
 * case the caller passes the image's natural size and the viewBox uses pixels.
 *
 * Front: hide-all masks every shape, the asked one in accent; hide-one masks only the asked one.
 * Back: the asked shapes open (dashed accent outline); in hide-all the others stay masked.
 * Text shapes are labels and always show.
 */
import type { OcclusionPayload, OcclusionShape } from '@core/types';

export type OcclusionSide = 'front' | 'back';
export interface OcclusionBox { width: number; height: number }

const n = (v: number | undefined) => {
  const x = Number.isFinite(v) ? (v as number) : 0;
  return String(Math.round(x * 100000) / 100000);
};
const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

/** Class for a mask shape, or null when it is not drawn on this side. */
export function maskState(shape: OcclusionShape, payload: Pick<OcclusionPayload, 'mode' | 'activeOrd'>, side: OcclusionSide): 'active' | 'revealed' | 'inactive' | null {
  const active = shape.ord === payload.activeOrd;
  if (side === 'front') {
    if (active) return 'active';
    return payload.mode === 'hide-all' ? 'inactive' : null;
  }
  if (active) return 'revealed';
  return payload.mode === 'hide-all' ? 'inactive' : null;
}

function shapeMarkup(s: OcclusionShape, cls: string, box: OcclusionBox): string {
  const label = s.text ? `<title>${esc(s.text)}</title>` : '';
  switch (s.kind) {
    case 'rect':
      return `<rect class="${cls}" x="${n(s.left)}" y="${n(s.top)}" width="${n(s.width)}" height="${n(s.height)}" rx="${n(Math.min(box.width, box.height) * 0.006)}">${label}</rect>`;
    case 'ellipse': {
      const w = s.width ?? 0, h = s.height ?? 0;
      return `<ellipse class="${cls}" cx="${n((s.left ?? 0) + w / 2)}" cy="${n((s.top ?? 0) + h / 2)}" rx="${n(w / 2)}" ry="${n(h / 2)}">${label}</ellipse>`;
    }
    case 'polygon': {
      const pts = (s.points ?? []).map(([x, y]) => `${n(x)},${n(y)}`).join(' ');
      return pts ? `<polygon class="${cls}" points="${pts}">${label}</polygon>` : '';
    }
    case 'text':
      return '';
  }
}

/**
 * SVG for the overlay. `box` is {1,1} for normalised coordinates, or the image's natural pixel size.
 * The SVG stretches over the image (preserveAspectRatio none), so strokes use non-scaling-stroke.
 */
export function occlusionSvg(payload: OcclusionPayload, side: OcclusionSide, box: OcclusionBox = { width: 1, height: 1 }, labels: { asked: string } = { asked: '' }): string {
  const parts: string[] = [];
  for (const s of payload.shapes) {
    if (s.kind === 'text') continue;
    const state = maskState(s, payload, side);
    if (!state) continue;
    const cls = `loci-io-mask${state === 'active' ? ' is-active' : state === 'revealed' ? ' is-revealed' : ''}`;
    parts.push(shapeMarkup(s, cls, box));
  }
  const aria = labels.asked ? ` role="img" aria-label="${esc(labels.asked)}"` : ' aria-hidden="true"';
  return `<svg class="loci-io-overlay" viewBox="0 0 ${n(box.width)} ${n(box.height)}" preserveAspectRatio="none"${aria}>${parts.join('')}</svg>`;
}

/** Text shapes as HTML labels positioned in percent (SVG text would stretch with the image). */
export function occlusionLabels(payload: OcclusionPayload, box: OcclusionBox = { width: 1, height: 1 }): string {
  return payload.shapes
    .filter((s) => s.kind === 'text' && s.text)
    .map((s) => {
      const x = ((s.left ?? 0) / (box.width || 1)) * 100;
      const y = ((s.top ?? 0) / (box.height || 1)) * 100;
      return `<span class="loci-io-text" dir="auto" style="left:${n(x)}%;top:${n(y)}%">${esc(s.text ?? '')}</span>`;
    })
    .join('');
}
