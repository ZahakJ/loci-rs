/**
 * Image Occlusion editor (DESIGN §10): pick or paste an image, draw rectangles, ellipses and
 * polygons (one card each), add text labels, select / move / resize, group shapes into one card,
 * switch hide-all / hide-one, zoom. Writes the Occlusion field in Anki's format and the Image
 * field as `<img src="name">`. Every shape is also listed below the canvas, so the whole editor
 * works from the keyboard and with a screen reader.
 */
import { useCallback, useEffect, useLayoutEffect, useReducer, useRef, useState, type KeyboardEvent, type PointerEvent as RPointerEvent } from 'react';
import { t } from '@core/i18n/i18n';
import { firstImageName, occlusionUnits } from '@core/template/occlusion';
import { resolveMediaRef } from '@ui/card/media';
import { errorMessage } from '@ui/api';
import { Icon, type IconName } from '@ui/design/icons';
import { Button, IconButton } from '@ui/design/primitives/Button';
import { Segmented } from '@ui/design/primitives/Segmented';
import { cx } from '@ui/design/primitives/layer';
import { toast } from '@ui/design/primitives/Toast';
import { storeMediaFile } from '@ui/editor/media';
import { hitTest, initialOcclusionState, occlusionReducer, serializeShapes, type EditShape, type Handle, type Tool } from './model';
import s from './OcclusionEditor.module.css';

export interface OcclusionEditorProps {
  occlusion: string;
  image: string;
  defaultMode?: 'hide-all' | 'hide-one';
  onChange: (next: { occlusion: string; image: string }) => void;
  error?: string;
}

type Draft =
  | { kind: 'box'; tool: 'rect' | 'ellipse'; x0: number; y0: number; x1: number; y1: number }
  | { kind: 'poly'; points: Array<[number, number]>; hover: [number, number] | null }
  | { kind: 'move'; ids: string[]; x: number; y: number; moved: boolean }
  | { kind: 'resize'; id: string; handle: Handle }
  | null;

const HANDLES: Handle[] = ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w'];
const TOOL_ICON: Record<Tool, IconName> = { select: 'cursor', rect: 'rect', ellipse: 'ellipse', polygon: 'polygon', text: 'text' };

export function OcclusionEditor({ occlusion, image, defaultMode, onChange, error }: OcclusionEditorProps) {
  const [state, dispatch] = useReducer(occlusionReducer, undefined, () => {
    const st = initialOcclusionState(occlusion);
    return occlusion.trim() ? st : { ...st, mode: defaultMode ?? 'hide-all' };
  });
  const imageName = firstImageName(image);
  const [url, setUrl] = useState('');
  const [natural, setNatural] = useState<{ w: number; h: number } | null>(null);
  const [zoom, setZoom] = useState<number | 'fit'>('fit');
  const [fitWidth, setFitWidth] = useState(0);
  const [draft, setDraft] = useState<Draft>(null);
  const [busy, setBusy] = useState(false);
  const [announce, setAnnounce] = useState('');
  const svg = useRef<SVGSVGElement>(null);
  const viewport = useRef<HTMLDivElement>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const lastOut = useRef(occlusion);
  const lastIn = useRef(occlusion);

  // external reset (note switched / fields cleared after Add)
  useEffect(() => {
    if (occlusion === lastOut.current || occlusion === lastIn.current) return;
    lastIn.current = occlusion;
    dispatch({ type: 'load', field: occlusion });
  }, [occlusion]);

  // state → field
  const serialized = serializeShapes(state);
  useEffect(() => {
    if (serialized === lastOut.current) return;
    if (!state.shapes.length && !lastOut.current.trim()) return;
    lastOut.current = serialized;
    onChange({ occlusion: serialized, image });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [serialized]);

  useEffect(() => {
    let live = true;
    setNatural(null);
    if (!imageName) { setUrl(''); return; }
    void resolveMediaRef(imageName).then((u) => { if (live) setUrl(u); });
    return () => { live = false; };
  }, [imageName]);

  useLayoutEffect(() => {
    const el = viewport.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => setFitWidth(e?.contentRect.width ?? 0));
    ro.observe(el);
    return () => ro.disconnect();
  }, [url]);

  const maxH = typeof window !== 'undefined' ? Math.max(260, window.innerHeight * 0.58) : 480;
  const fitScale = natural ? Math.min(fitWidth / natural.w, maxH / natural.h, 2) : 1;
  const scale = zoom === 'fit' ? fitScale : zoom;
  const dispW = natural ? natural.w * scale : fitWidth;
  const dispH = natural ? natural.h * scale : 0;

  const setImage = useCallback(async (file: File) => {
    if (!file.type.startsWith('image/')) return;
    setBusy(true);
    try {
      const name = await storeMediaFile(file);
      const img = `<img src="${name.replace(/"/g, '&quot;')}">`;
      onChange({ occlusion: serializeShapes(state), image: img });
      setZoom('fit');
    } catch (e) {
      toast({ message: t('editor.mediaFailed', { message: errorMessage(e) }), tone: 'danger' });
    } finally { setBusy(false); }
  }, [onChange, state]);

  // paste an image anywhere while the editor has no image, or into the canvas
  useEffect(() => {
    const onPaste = (e: ClipboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (target && target.closest('[contenteditable], input, textarea') && !target.closest(`.${s.root}`)) return;
      const file = Array.from(e.clipboardData?.files ?? []).find((f) => f.type.startsWith('image/'));
      if (!file) return;
      if (imageName && !(target && target.closest(`.${s.root}`))) return;
      e.preventDefault();
      void setImage(file);
    };
    document.addEventListener('paste', onPaste);
    return () => document.removeEventListener('paste', onPaste);
  }, [imageName, setImage]);

  // ── pointer geometry ──
  const point = (e: { clientX: number; clientY: number }): [number, number] => {
    const r = svg.current!.getBoundingClientRect();
    return [Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)), Math.min(1, Math.max(0, (e.clientY - r.top) / r.height))];
  };

  const finishPolygon = (pts: Array<[number, number]>) => {
    if (pts.length >= 3) dispatch({ type: 'add', shape: { kind: 'polygon', left: 0, top: 0, width: 0, height: 0, points: pts } });
    setDraft(null);
  };

  const onPointerDown = (e: RPointerEvent<SVGSVGElement>) => {
    if (e.button !== 0) return;
    const [x, y] = point(e);
    const handle = (e.target as Element).getAttribute('data-handle') as Handle | null;
    const sel = state.selected.length === 1 ? state.shapes.find((sh) => sh.id === state.selected[0]) : undefined;
    if (handle && sel) {
      svg.current!.setPointerCapture(e.pointerId);
      setDraft({ kind: 'resize', id: sel.id, handle });
      return;
    }
    if (state.tool === 'polygon') {
      const pts = draft?.kind === 'poly' ? draft.points : [];
      const first = pts[0];
      if (first && pts.length >= 3 && Math.hypot((x - first[0]) * dispW, (y - first[1]) * dispH) < 10) { finishPolygon(pts); return; }
      setDraft({ kind: 'poly', points: [...pts, [x, y]], hover: null });
      return;
    }
    if (state.tool === 'text') {
      dispatch({ type: 'add', shape: { kind: 'text', left: x, top: y, width: 0.12, height: 0.05, text: t('editor.occlusion.labelDefault') } });
      dispatch({ type: 'tool', tool: 'select' });
      requestAnimationFrame(() => document.getElementById('io-label-input')?.focus());
      return;
    }
    if (state.tool === 'rect' || state.tool === 'ellipse') {
      svg.current!.setPointerCapture(e.pointerId);
      setDraft({ kind: 'box', tool: state.tool, x0: x, y0: y, x1: x, y1: y });
      return;
    }
    // select tool
    const hit = hitTest(state.shapes, x, y);
    if (!hit) { dispatch({ type: 'select', ids: [] }); return; }
    const additive = e.shiftKey || e.ctrlKey || e.metaKey;
    const ids = additive ? state.selected : state.selected.includes(hit.id) ? state.selected : [hit.id];
    if (additive || !state.selected.includes(hit.id)) dispatch({ type: 'select', ids: [hit.id], additive });
    svg.current!.setPointerCapture(e.pointerId);
    setDraft({ kind: 'move', ids: additive ? [...ids, hit.id] : ids, x, y, moved: false });
  };

  const onPointerMove = (e: RPointerEvent<SVGSVGElement>) => {
    if (!draft) return;
    const [x, y] = point(e);
    if (draft.kind === 'box') setDraft({ ...draft, x1: x, y1: y });
    else if (draft.kind === 'poly') setDraft({ ...draft, hover: [x, y] });
    else if (draft.kind === 'move') {
      dispatch({ type: 'move', ids: draft.ids, dx: x - draft.x, dy: y - draft.y });
      setDraft({ ...draft, x, y, moved: true });
    } else if (draft.kind === 'resize') dispatch({ type: 'resize', id: draft.id, handle: draft.handle, x, y });
  };

  const onPointerUp = () => {
    if (!draft) return;
    if (draft.kind === 'box') {
      const left = Math.min(draft.x0, draft.x1), top = Math.min(draft.y0, draft.y1);
      const width = Math.abs(draft.x1 - draft.x0), height = Math.abs(draft.y1 - draft.y0);
      const before = state.shapes.length;
      dispatch({ type: 'add', shape: { kind: draft.tool, left, top, width, height } });
      if (width > 0.01 && height > 0.01) setAnnounce(t('editor.occlusion.added', { n: before + 1 }));
      setDraft(null);
    } else if (draft.kind !== 'poly') setDraft(null);
  };

  const onCanvasKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'Escape' && draft?.kind === 'poly') { e.preventDefault(); e.stopPropagation(); setDraft(null); return; }
    if (e.key === 'Enter' && draft?.kind === 'poly') { e.preventDefault(); finishPolygon(draft.points); return; }
    if ((e.key === 'Delete' || e.key === 'Backspace') && state.selected.length) { e.preventDefault(); removeSelected(); return; }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'a') { e.preventDefault(); dispatch({ type: 'selectAll' }); return; }
    const step = e.shiftKey ? 0.05 : 0.005;
    const d: Record<string, [number, number]> = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] };
    const v = d[e.key];
    if (v && state.selected.length) { e.preventDefault(); dispatch({ type: 'move', ids: state.selected, dx: v[0], dy: v[1] }); return; }
    const keys: Record<string, Tool> = { v: 'select', r: 'rect', e: 'ellipse', p: 'polygon', t: 'text' };
    const tool = !e.ctrlKey && !e.metaKey && !e.altKey ? keys[e.key.toLowerCase()] : undefined;
    if (tool) { e.preventDefault(); dispatch({ type: 'tool', tool }); }
  };

  const removeSelected = () => {
    const n = state.selected.length;
    dispatch({ type: 'delete', ids: state.selected });
    setAnnounce(t('editor.occlusion.deleted', { count: n }));
  };

  // legacy pixel coordinates → normalised, once the natural size is known
  useEffect(() => {
    if (natural && state.shapes.length && occlusionUnits(state.shapes) === 'pixels') dispatch({ type: 'normalize', width: natural.w, height: natural.h });
  }, [natural, state.shapes]);

  const selectedShape = state.selected.length === 1 ? state.shapes.find((sh) => sh.id === state.selected[0]) : undefined;
  const cards = new Set(state.shapes.filter((sh) => sh.kind !== 'text').map((sh) => sh.ord)).size;

  if (!imageName) {
    return (
      <div className={cx(s.root, s.empty)} data-dragging={busy || undefined}
        onDragOver={(e) => e.preventDefault()}
        onDrop={(e) => { e.preventDefault(); const f = Array.from(e.dataTransfer.files).find((x) => x.type.startsWith('image/')); if (f) void setImage(f); }}
      >
        <span className={s.emptyMark} aria-hidden><Icon name="occlusion" size={26} /></span>
        <p className={s.emptyTitle}>{t('editor.occlusion.pickImage')}</p>
        <p className={s.emptyHint}>{t('editor.occlusion.pickHint')}</p>
        <Button variant="primary" icon="image" loading={busy} onClick={() => fileInput.current?.click()}>{t('editor.occlusion.choose')}</Button>
        <input ref={fileInput} type="file" accept="image/*" hidden onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ''; if (f) void setImage(f); }} />
        {error && <p className={s.error} role="alert">{error}</p>}
      </div>
    );
  }

  const px = (v: number) => v * dispW, py = (v: number) => v * dispH;
  const renderShape = (sh: EditShape, extra?: string) => {
    const cls = cx(s.shape, sh.kind === 'text' && s.textShape, state.selected.includes(sh.id) && s.selected, extra);
    switch (sh.kind) {
      case 'rect': return <rect key={sh.id} className={cls} x={px(sh.left)} y={py(sh.top)} width={px(sh.width)} height={py(sh.height)} rx={3} />;
      case 'ellipse': return <ellipse key={sh.id} className={cls} cx={px(sh.left + sh.width / 2)} cy={py(sh.top + sh.height / 2)} rx={px(sh.width / 2)} ry={py(sh.height / 2)} />;
      case 'polygon': return <polygon key={sh.id} className={cls} points={(sh.points ?? []).map(([x, y]) => `${px(x)},${py(y)}`).join(' ')} />;
      case 'text': return (
        <g key={sh.id} className={cls}>
          <rect className={s.textBox} x={px(sh.left) - 4} y={py(sh.top) - 2} width={Math.max(40, (sh.text?.length ?? 4) * 8 + 8)} height={22} rx={4} />
          <text className={s.textLabel} x={px(sh.left)} y={py(sh.top) + 14}>{sh.text}</text>
        </g>
      );
    }
  };

  return (
    <div className={s.root}>
      <div className={s.toolbar} role="toolbar" aria-label={t('editor.occlusion.tools')}>
        <div className={s.tools} role="radiogroup" aria-label={t('editor.occlusion.tool')}>
          {(['select', 'rect', 'ellipse', 'polygon', 'text'] as Tool[]).map((tool) => (
            <button
              key={tool}
              type="button"
              role="radio"
              aria-checked={state.tool === tool}
              className={s.tool}
              title={`${t(`editor.occlusion.${tool === 'rect' ? 'rectangle' : tool}`)} (${({ select: 'V', rect: 'R', ellipse: 'E', polygon: 'P', text: 'T' } as const)[tool]})`}
              aria-label={t(`editor.occlusion.${tool === 'rect' ? 'rectangle' : tool}`)}
              onClick={() => { dispatch({ type: 'tool', tool }); setDraft(null); }}
            >
              <Icon name={TOOL_ICON[tool]} size={18} />
            </button>
          ))}
        </div>
        <span className={s.sep} aria-hidden />
        <IconButton size="sm" icon="group" label={t('editor.occlusion.group')} disabled={state.selected.length < 2} onClick={() => dispatch({ type: 'group', ids: state.selected })} />
        <IconButton size="sm" icon="trash" label={t('editor.occlusion.deleteShape')} disabled={!state.selected.length} onClick={removeSelected} />
        <span className={s.sep} aria-hidden />
        <IconButton size="sm" icon="zoom-out" label={t('editor.occlusion.zoomOut')} onClick={() => setZoom(Math.max(0.1, scale / 1.25))} />
        <IconButton size="sm" icon="zoom-fit" label={t('editor.occlusion.zoomFit')} aria-pressed={zoom === 'fit'} onClick={() => setZoom('fit')} />
        <IconButton size="sm" icon="zoom-in" label={t('editor.occlusion.zoomIn')} onClick={() => setZoom(Math.min(6, scale * 1.25))} />
        <span className={s.zoom} aria-live="polite">{Math.round(scale * 100)}%</span>
      </div>

      <div className={s.mode}>
        <Segmented
          label={t('editor.occlusion.mode')}
          value={state.mode}
          onChange={(mode) => dispatch({ type: 'mode', mode })}
          options={[
            { value: 'hide-all', label: t('editor.occlusion.hideAll') },
            { value: 'hide-one', label: t('editor.occlusion.hideOne') },
          ]}
        />
        <span className={s.count}>{t('editor.occlusion.cards', { count: cards })}</span>
      </div>

      <div
        ref={viewport}
        className={s.viewport}
        tabIndex={0}
        role="application"
        aria-label={t('editor.occlusion.canvas')}
        aria-describedby="io-help"
        onKeyDown={onCanvasKey}
        onDoubleClick={() => { if (draft?.kind === 'poly') finishPolygon(draft.points); }}
      >
        <div className={s.stage} style={{ inlineSize: dispW || undefined, blockSize: dispH || undefined }}>
          {url && (
            <img
              src={url}
              alt=""
              className={s.image}
              draggable={false}
              onLoad={(e) => setNatural({ w: e.currentTarget.naturalWidth || 800, h: e.currentTarget.naturalHeight || 600 })}
              style={{ inlineSize: dispW || '100%', blockSize: dispH || 'auto' }}
            />
          )}
          {natural && (
            <svg
              ref={svg}
              className={cx(s.overlay, s[`tool-${state.tool}`])}
              width={dispW}
              height={dispH}
              viewBox={`0 0 ${dispW} ${dispH}`}
              onPointerDown={onPointerDown}
              onPointerMove={onPointerMove}
              onPointerUp={onPointerUp}
              onPointerCancel={() => setDraft(null)}
              aria-hidden
            >
              {state.shapes.map((sh) => renderShape(sh))}
              {state.shapes.filter((sh) => sh.kind !== 'text').map((sh) => (
                <text key={`n${sh.id}`} className={s.ordBadge} x={px(sh.left + sh.width / 2)} y={py(sh.top + sh.height / 2) + 5} textAnchor="middle">{sh.ord}</text>
              ))}
              {draft?.kind === 'box' && (draft.tool === 'rect'
                ? <rect className={cx(s.shape, s.draft)} x={px(Math.min(draft.x0, draft.x1))} y={py(Math.min(draft.y0, draft.y1))} width={px(Math.abs(draft.x1 - draft.x0))} height={py(Math.abs(draft.y1 - draft.y0))} rx={3} />
                : <ellipse className={cx(s.shape, s.draft)} cx={px((draft.x0 + draft.x1) / 2)} cy={py((draft.y0 + draft.y1) / 2)} rx={px(Math.abs(draft.x1 - draft.x0) / 2)} ry={py(Math.abs(draft.y1 - draft.y0) / 2)} />)}
              {draft?.kind === 'poly' && (
                <>
                  <polyline className={cx(s.shape, s.draft)} points={[...draft.points, ...(draft.hover ? [draft.hover] : [])].map(([x, y]) => `${px(x)},${py(y)}`).join(' ')} />
                  {draft.points.map(([x, y], i) => <circle key={i} className={cx(s.vertex, i === 0 && s.vertexFirst)} cx={px(x)} cy={py(y)} r={i === 0 ? 6 : 4} />)}
                </>
              )}
              {selectedShape && selectedShape.kind !== 'text' && state.tool === 'select' && (
                <g>
                  <rect className={s.bbox} x={px(selectedShape.left)} y={py(selectedShape.top)} width={px(selectedShape.width)} height={py(selectedShape.height)} />
                  {HANDLES.map((h) => {
                    const hx = selectedShape.left + (h.includes('w') ? 0 : h.includes('e') ? selectedShape.width : selectedShape.width / 2);
                    const hy = selectedShape.top + (h.includes('n') ? 0 : h.includes('s') ? selectedShape.height : selectedShape.height / 2);
                    return <rect key={h} data-handle={h} className={cx(s.handle, s[`h-${h}`])} x={px(hx) - 5} y={py(hy) - 5} width={10} height={10} rx={2} />;
                  })}
                </g>
              )}
            </svg>
          )}
        </div>
      </div>
      <p id="io-help" className={s.help}>{draft?.kind === 'poly' ? t('editor.occlusion.polygonHelp') : t('editor.occlusion.help')}</p>

      {selectedShape?.kind === 'text' && (
        <label className={s.labelEdit}>
          <span>{t('editor.occlusion.text')}</span>
          <input id="io-label-input" className={s.labelInput} value={selectedShape.text ?? ''} onChange={(e) => dispatch({ type: 'label', id: selectedShape.id, text: e.target.value })} dir="auto" />
        </label>
      )}

      {state.shapes.length > 0 && (
        <ul role="list" className={s.list} aria-label={t('editor.occlusion.shapes')}>
          {state.shapes.map((sh) => (
            <li key={sh.id} className={s.item} data-selected={state.selected.includes(sh.id) || undefined}>
              <button type="button" className={s.itemBtn} aria-pressed={state.selected.includes(sh.id)} onClick={(e) => { dispatch({ type: 'tool', tool: 'select' }); dispatch({ type: 'select', ids: [sh.id], additive: e.shiftKey || e.ctrlKey || e.metaKey }); }}>
                <Icon name={TOOL_ICON[sh.kind]} size={16} />
                <span>{sh.kind === 'text' ? `“${sh.text ?? ''}”` : t('editor.occlusion.shapeCard', { n: sh.ord })}</span>
              </button>
              <IconButton size="sm" icon="close" label={t('editor.occlusion.deleteShape')} onClick={() => dispatch({ type: 'delete', ids: [sh.id] })} />
            </li>
          ))}
        </ul>
      )}
      <div className={s.footer}>
        <Button size="sm" variant="ghost" icon="image" loading={busy} onClick={() => fileInput.current?.click()}>{t('editor.occlusion.changeImage')}</Button>
        <input ref={fileInput} type="file" accept="image/*" hidden onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ''; if (f) void setImage(f); }} />
      </div>
      {error && <p className={s.error} role="alert">{error}</p>}
      <span className="sr-only" aria-live="polite">{announce}</span>
    </div>
  );
}
