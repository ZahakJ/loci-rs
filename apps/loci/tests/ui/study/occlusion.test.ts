import { describe, expect, it } from 'vitest';
import type { OcclusionPayload } from '@core/types';
import { maskState, occlusionLabels, occlusionSvg } from '@ui/card/occlusionSvg';

const payload: OcclusionPayload = {
  image: 'x.png', mode: 'hide-all', activeOrd: 2,
  shapes: [
    { kind: 'rect', ord: 1, left: 0.1, top: 0.1, width: 0.2, height: 0.1 },
    { kind: 'ellipse', ord: 2, left: 0.5, top: 0.5, width: 0.2, height: 0.1 },
    { kind: 'polygon', ord: 3, points: [[0, 0], [0.1, 0], [0.1, 0.1]] },
    { kind: 'text', ord: 0, left: 0.3, top: 0.4, text: 'A<b>' },
  ],
};

describe('image occlusion overlay', () => {
  it('hide-all masks everything on the front, asked one active', () => {
    expect(maskState(payload.shapes[0]!, payload, 'front')).toBe('inactive');
    expect(maskState(payload.shapes[1]!, payload, 'front')).toBe('active');
    const svg = occlusionSvg(payload, 'front');
    expect(svg.match(/loci-io-mask/g)).toHaveLength(3);
    expect(svg).toContain('<ellipse class="loci-io-mask is-active" cx="0.6" cy="0.55" rx="0.1" ry="0.05"');
  });
  it('hide-one masks only the asked shape', () => {
    const p = { ...payload, mode: 'hide-one' as const };
    expect(occlusionSvg(p, 'front').match(/loci-io-mask/g)).toHaveLength(1);
    expect(occlusionSvg(p, 'back')).toContain('is-revealed');
    expect(occlusionSvg(p, 'back').match(/loci-io-mask/g)).toHaveLength(1);
  });
  it('reveals the asked shape on the back, others stay masked in hide-all', () => {
    const svg = occlusionSvg(payload, 'back');
    expect(svg).toContain('loci-io-mask is-revealed');
    expect(svg.match(/class="loci-io-mask"/g)).toHaveLength(2);
  });
  it('uses pixel boxes for legacy notes and escapes labels', () => {
    expect(occlusionSvg(payload, 'front', { width: 800, height: 600 })).toContain('viewBox="0 0 800 600"');
    expect(occlusionLabels(payload)).toBe('<span class="loci-io-text" dir="auto" style="left:30%;top:40%">A&lt;b&gt;</span>');
  });
});
