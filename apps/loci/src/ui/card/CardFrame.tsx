/**
 * CardFrame: renders one side of a RenderedCard inside a Shadow DOM host (internals §2, §6).
 *
 * Pipeline per render: `loci-media:` URLs → object URLs, DOMPurify with Loci's card config,
 * insert into `.card` in the shadow root, then enhance: answer wrapper (for the reveal motion),
 * typed-answer box / diff, image-occlusion overlay, one-by-one cloze masks, KaTeX, audio buttons,
 * hint links, image zoom. Card CSS cannot reach the app and app CSS cannot reach the card; the
 * `--loci-*` variables (and the design tokens) inherit through the host.
 */
import { useEffect, useImperativeHandle, useLayoutEffect, useRef, useState, type Ref } from 'react';
import DOMPurify from 'dompurify';
import { t } from '@core/i18n/i18n';
import type { RenderedCard } from '@core/types';
import { LOCI_CARD_BASE_CSS } from '@core/template/css';
import { sanitizeCardHtml, type HtmlSanitizer } from '@core/template/render';
import { CARD_SANITIZE_CONFIG } from '@core/template/sanitize-config';
import { compareTypedAnswer } from '@core/template/typed';
import { occlusionUnits } from '@core/template/occlusion';
import { cx } from '../design/primitives/layer';
import { useUi } from '../store/ui';
import { playAudio, stopAudio } from './audio';
import { katexSheet, renderMath } from './math';
import { resolveMediaIn, resolveMediaRef, resolveMediaUrl } from './media';
import { occlusionLabels, occlusionSvg } from './occlusionSvg';
import { Lightbox } from './Lightbox';
import { answerAudio } from '../views/study/format';
import s from './CardFrame.module.css';

export type CardSide = 'front' | 'back';

export interface CardFrameHandle {
  /** replays the current side's audio ('r') */
  replay: () => void;
  /** focuses the typed-answer box; false when the side has none ('t') */
  focusTyped: () => boolean;
  stopAudio: () => void;
}

export interface CardRenderInfo {
  side: CardSide;
  hasTypeInput: boolean;
  hasAudio: boolean;
  /** one-by-one cloze spans on this side */
  oneByOne: number;
}

export interface CardFrameProps {
  card: RenderedCard;
  side: CardSide;
  /** typed answer (controlled by the parent; read when the back renders) */
  typed?: string;
  onTypedChange?: (value: string) => void;
  /** Enter in the typed-answer box */
  onTypedSubmit?: () => void;
  /** the typed-answer box gained or lost focus (the answer bar shows 'Check' while it has it) */
  onTypedFocus?: (focused: boolean) => void;
  typedOptions?: { ignoreCase: boolean; ignoreDiacritics: boolean };
  /** one-by-one: how many of the back's active clozes are revealed (undefined = all) */
  clozeRevealed?: number;
  onClozeClick?: (index: number) => void;
  /** play this side's audio after it renders */
  autoplay?: boolean;
  /** on the answer, also replay the question's audio */
  replayQuestionAudio?: boolean;
  /** animate the answer in (hairline draws, answer rises) */
  animateReveal?: boolean;
  onRendered?: (info: CardRenderInfo) => void;
  className?: string;
  ref?: Ref<CardFrameHandle>;
}

// ── shared stylesheets ──
let baseSheet: CSSStyleSheet | null = null;
let frameSheet: CSSStyleSheet | null = null;

/** Loci additions inside the shadow root (tokens inherit from the host document). */
const FRAME_CSS = `
.loci-page { display: block; }
.card { outline: none; }
/* short viewports (phones in landscape): an image never takes more than about half the height */
@media (max-height: 520px) {
  .card img:not([data-io]) { max-block-size: 45vh; inline-size: auto; max-inline-size: 100%; object-fit: contain; }
  .card .loci-io { max-inline-size: min(100%, calc(45vh * var(--loci-io-ratio, 1.333))); margin-inline: auto; }
}
.card [data-shape], .card #image-occlusion-canvas { display: none !important; }
.card img:not([data-io]) { cursor: zoom-in; }
.card a[href^="http"]::after { content: "\\2197"; font-size: .7em; margin-inline-start: .1em; color: var(--_muted); }

/* typed answer */
.loci-type-input {
  display: block; box-sizing: border-box; inline-size: 100%; max-inline-size: 26rem;
  margin: .9em auto 0; padding: 0 .9rem; block-size: 48px;
  font: 400 1rem/1.4 var(--font-ui, system-ui, sans-serif); color: var(--ink, var(--_ink));
  background: var(--surface, var(--_surface)); border: 1px solid var(--line-strong, var(--_line));
  border-radius: var(--r-md, 10px); text-align: start; unicode-bidi: plaintext;
  transition: border-color var(--d-fast, 120ms) var(--ease, ease);
}
.card[data-align="start"] .loci-type-input { margin-inline: 0; }
.loci-type-input::placeholder { color: var(--faint, var(--_muted)); }
.loci-type-input:hover { border-color: var(--muted, var(--_muted)); }
.loci-type-input:focus { outline: 2px solid var(--focus, var(--_accent)); outline-offset: 2px; border-color: var(--line-strong, var(--_line)); }
.loci-typed { display: block; margin: .9em 0 0; }
.loci-typed-verdict {
  display: block; margin-block-end: .35em;
  font: 600 var(--text-xs, .75rem)/1.4 var(--font-ui, system-ui, sans-serif); letter-spacing: .04em; text-transform: uppercase;
}
.loci-typed-verdict.is-correct { color: var(--good, var(--_good)); }
.loci-typed-verdict.is-wrong { color: var(--muted, var(--_muted)); }
.card #typeans { font-size: .8em; }

/* image occlusion */
.card .loci-io { position: relative; display: block; inline-size: 100%; line-height: 0; margin-block: .25em; }
.card .loci-io img { display: block; inline-size: 100%; max-inline-size: 100%; block-size: auto; border-radius: var(--r-sm, 6px); cursor: default; }
.card .loci-io > svg.loci-io-overlay { position: absolute; inset: 0; inline-size: 100%; block-size: 100%; overflow: visible; }
.card .loci-io-mask { fill: var(--loci-io-fill); stroke: var(--loci-io-stroke); stroke-width: var(--loci-io-stroke-width, 1px); vector-effect: non-scaling-stroke; transition: fill var(--d-base, 200ms) var(--ease, ease), stroke var(--d-base, 200ms) var(--ease, ease); }
.card .loci-io-mask.is-active { fill: var(--loci-io-active); stroke: var(--loci-io-active-stroke, var(--_accent)); stroke-width: 2px; }
.card .loci-io-mask.is-revealed { fill: transparent; stroke: var(--accent, var(--_accent)); stroke-width: 2px; stroke-dasharray: 5 4; }
.card .loci-io-text {
  position: absolute; line-height: 1.3; padding: .05em .35em; border-radius: 4px; white-space: nowrap;
  font: 600 max(11px, .55em)/1.3 var(--font-ui, system-ui, sans-serif); color: var(--ink, var(--_ink));
  background: color-mix(in oklab, var(--surface, var(--_surface)) 88%, transparent);
}
.card .loci-io-header { font-family: var(--font-ui, system-ui, sans-serif); font-size: .62em; font-weight: 600; letter-spacing: .02em; color: var(--muted, var(--_muted)); margin-block-end: .7em; }

/* one-by-one */
.card .cloze[data-cloze-index] { transition: color var(--d-fast, 120ms) var(--ease, ease), background-color var(--d-fast, 120ms) var(--ease, ease); border-radius: 4px; }
.card .cloze[data-cloze-index].loci-hidden { color: transparent; background: color-mix(in oklch, var(--_cloze) 14%, transparent); cursor: pointer; }
.card .cloze[data-cloze-index].loci-next { box-shadow: 0 0 0 1.5px color-mix(in oklch, var(--_cloze) 55%, transparent); }

/* answer reveal: the hairline draws from the centre while the answer rises in */
.card.is-revealing hr#answer { animation: loci-draw var(--d-base, 200ms) var(--ease, ease) both; }
.card.is-revealing .loci-answer { animation: loci-answer-in calc(var(--d-base, 200ms) * 1.2) var(--ease, ease) both; }
.card.is-revealing.no-divider .cloze, .card.is-revealing.no-divider .loci-typed, .card.is-revealing .loci-io-mask.is-revealed { animation: loci-fade calc(var(--d-base, 200ms) * 1.2) var(--ease, ease) both; }
@keyframes loci-draw { from { transform: scaleX(0); } to { transform: scaleX(1); } }
@keyframes loci-answer-in { from { opacity: 0; transform: translateY(8px); } to { opacity: 1; transform: none; } }
@keyframes loci-fade { from { opacity: 0; } to { opacity: 1; } }
@media (prefers-reduced-motion: reduce) {
  .card.is-revealing .loci-answer { animation-name: loci-fade; }
  .card.is-revealing hr#answer { animation-name: loci-fade; }
}
`;

function sheets(): { base: CSSStyleSheet; frame: CSSStyleSheet } {
  if (!baseSheet) { baseSheet = new CSSStyleSheet(); baseSheet.replaceSync(LOCI_CARD_BASE_CSS); }
  if (!frameSheet) { frameSheet = new CSSStyleSheet(); frameSheet.replaceSync(FRAME_CSS); }
  return { base: baseSheet, frame: frameSheet };
}

const purify = DOMPurify as unknown as HtmlSanitizer;

/** Sanitised card HTML with media resolved (exported for previews that render outside a frame). */
export async function prepareCardHtml(html: string): Promise<string> {
  return sanitizeCardHtml(await resolveMediaIn(html), purify);
}

/** Wraps everything after `hr#answer` (at its level) in `.loci-answer`; false when there is no divider. */
function wrapAnswer(root: HTMLElement): boolean {
  const hr = root.querySelector('hr#answer');
  if (!hr || !hr.parentNode) return false;
  const wrap = document.createElement('div');
  wrap.className = 'loci-answer';
  let n = hr.nextSibling;
  while (n) { const next = n.nextSibling; wrap.appendChild(n); n = next; }
  hr.parentNode.appendChild(wrap);
  return true;
}

function typeSlot(root: HTMLElement): HTMLElement | null {
  return root.querySelector<HTMLElement>('.loci-type') ?? root.querySelector<HTMLElement>('.loci-type-slot');
}

const loadImage = (img: HTMLImageElement) => new Promise<void>((res) => {
  if (img.complete && img.naturalWidth) { res(); return; }
  img.addEventListener('load', () => res(), { once: true });
  img.addEventListener('error', () => res(), { once: true });
});

export function CardFrame({
  card, side, typed = '', onTypedChange, onTypedSubmit, onTypedFocus, typedOptions, clozeRevealed, onClozeClick,
  autoplay, replayQuestionAudio = true, animateReveal, onRendered, className, ref,
}: CardFrameProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);
  const pageRef = useRef<HTMLDivElement | null>(null);
  const cssSheetRef = useRef<CSSStyleSheet | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const tokenRef = useRef(0);
  const [zoom, setZoom] = useState<{ src: string; alt: string } | null>(null);
  const theme = useUi((st) => st.theme);

  // Latest callbacks/values without re-rendering the card.
  const live = useRef({ typed, onTypedChange, onTypedSubmit, onTypedFocus, typedOptions, onClozeClick, onRendered, clozeRevealed, replayQuestionAudio });
  useLayoutEffect(() => { live.current = { typed, onTypedChange, onTypedSubmit, onTypedFocus, typedOptions, onClozeClick, onRendered, clozeRevealed, replayQuestionAudio }; });

  // Shadow root, once.
  useLayoutEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const root = host.shadowRoot ?? host.attachShadow({ mode: 'open' });
    if (!contentRef.current) {
      // Anki puts .nightMode on <body> around .card; the wrapper plays <body> so both
      // `.nightMode .card` and `.card.nightMode` selectors in imported CSS match.
      const page = document.createElement('div');
      page.className = 'loci-page';
      const content = document.createElement('div');
      content.className = 'card';
      page.append(content);
      root.replaceChildren(page);
      pageRef.current = page;
      contentRef.current = content;
      cssSheetRef.current = new CSSStyleSheet();
      const { base, frame } = sheets();
      root.adoptedStyleSheets = [base, katexSheet(), cssSheetRef.current, frame];
      root.addEventListener('click', (e) => onShadowClick(e as MouseEvent));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const audioList = (which: CardSide): string[] => which === 'front'
    ? card.front.media
    : (() => { const a = answerAudio(card.front.media, card.back.media, live.current.replayQuestionAudio); return a.length ? a : card.back.media; })();

  const playNames = async (names: string[]) => {
    const root = contentRef.current;
    const urls = await Promise.all(names.map((n) => resolveMediaUrl(n)));
    playAudio(names.map((n, i) => ({ url: urls[i] ?? '', button: root?.querySelector<HTMLElement>(`.loci-audio[data-name="${CSS.escape(n)}"]`) ?? null })));
  };

  function onShadowClick(e: MouseEvent) {
    const target = e.target as HTMLElement | null;
    if (!target) return;
    const audioBtn = target.closest<HTMLElement>('.loci-audio');
    if (audioBtn) {
      e.preventDefault();
      const src = audioBtn.getAttribute('data-src') ?? '';
      if (src && !src.startsWith('loci-media:')) playAudio([{ url: src, button: audioBtn }]);
      else {
        const name = audioBtn.getAttribute('data-name') ?? '';
        void resolveMediaUrl(name).then((url) => playAudio([{ url, button: audioBtn }]));
      }
      return;
    }
    const hint = target.closest<HTMLAnchorElement>('a.hint[data-hint]');
    if (hint) {
      e.preventDefault();
      const div = contentRef.current?.querySelector<HTMLElement>(`#${CSS.escape(hint.dataset.hint ?? '')}`);
      if (div) { div.style.display = 'block'; hint.style.display = 'none'; }
      return;
    }
    const hidden = target.closest<HTMLElement>('.cloze[data-cloze-index].loci-hidden');
    if (hidden) {
      live.current.onClozeClick?.(Number(hidden.dataset.clozeIndex ?? 0));
      return;
    }
    const img = target.closest<HTMLImageElement>('img');
    if (img && !img.hasAttribute('data-io') && !img.closest('a') && img.currentSrc) {
      setZoom({ src: img.currentSrc || img.src, alt: img.alt });
    }
  }

  // Render on card/side change.
  useEffect(() => {
    const content = contentRef.current;
    if (!content) return;
    const token = ++tokenRef.current;
    const sideData = side === 'front' ? card.front : card.back;
    let cancelled = false;
    void (async () => {
      const [html, css, ioImage] = await Promise.all([
        prepareCardHtml(sideData.html),
        resolveMediaIn(card.css ?? ''),
        card.occlusion ? resolveMediaRef(card.occlusion.image) : Promise.resolve(''),
      ]);
      if (cancelled || token !== tokenRef.current) return;
      try { cssSheetRef.current?.replaceSync(css); } catch { /* invalid card CSS is ignored */ }
      content.innerHTML = html;
      content.setAttribute('dir', card.dir === 'auto' ? 'auto' : card.dir);
      content.classList.remove('is-revealing', 'no-divider');
      const hasDivider = side === 'back' ? wrapAnswer(content) : false;
      if (side === 'back' && !hasDivider) content.classList.add('no-divider');

      for (const a of Array.from(content.querySelectorAll<HTMLAnchorElement>('a[href^="http"], a[href^="mailto"]'))) {
        a.target = '_blank';
        a.rel = 'noopener noreferrer';
      }
      for (const b of Array.from(content.querySelectorAll<HTMLButtonElement>('.loci-audio'))) {
        b.type = 'button';
        if (!b.getAttribute('aria-label')) b.setAttribute('aria-label', t('study.playAudio'));
      }

      // typed answer
      if (inputRef.current) live.current.onTypedFocus?.(false);
      inputRef.current = null;
      const slot = sideData.hasTypeInput ? typeSlot(content) : null;
      if (slot) {
        if (side === 'front') {
          const input = document.createElement('input');
          input.type = 'text';
          input.className = 'loci-type-input';
          input.dir = 'auto';
          input.autocomplete = 'off';
          input.spellcheck = false;
          input.setAttribute('autocorrect', 'off');
          input.setAttribute('autocapitalize', 'off');
          input.setAttribute('enterkeyhint', 'done');
          input.placeholder = t('study.typeAnswerPlaceholder');
          input.setAttribute('aria-label', t('study.typeAnswerPlaceholder'));
          input.value = live.current.typed;
          input.addEventListener('input', () => live.current.onTypedChange?.(input.value));
          input.addEventListener('focus', () => live.current.onTypedFocus?.(true));
          input.addEventListener('blur', () => live.current.onTypedFocus?.(false));
          input.addEventListener('keydown', (ev) => {
            if (ev.key === 'Enter' && !ev.isComposing) { ev.preventDefault(); ev.stopPropagation(); live.current.onTypedSubmit?.(); return; }
            if (ev.key === 'Escape') { ev.preventDefault(); ev.stopPropagation(); input.blur(); hostRef.current?.closest<HTMLElement>('[data-study-stage]')?.focus(); return; }
            ev.stopPropagation();
          });
          slot.replaceWith(input);
          inputRef.current = input;
        } else {
          const expected = slot.getAttribute('data-answer') ?? sideData.typeAnswer ?? card.front.typeAnswer ?? '';
          const opts = live.current.typedOptions ?? { ignoreCase: false, ignoreDiacritics: false };
          const res = compareTypedAnswer(expected, live.current.typed, {
            ignoreCase: opts.ignoreCase || slot.hasAttribute('data-ci'),
            ignoreDiacritics: opts.ignoreDiacritics || slot.hasAttribute('data-nc'),
          });
          const wrap = document.createElement('div');
          wrap.className = 'loci-typed';
          const verdict = document.createElement('span');
          verdict.className = `loci-typed-verdict ${res.correct ? 'is-correct' : 'is-wrong'}`;
          verdict.textContent = res.correct ? t('study.typedCorrect') : live.current.typed.trim() ? t('study.typedIncorrect') : t('cardFrame.typedEmpty');
          wrap.append(verdict);
          wrap.insertAdjacentHTML('beforeend', DOMPurify.sanitize(res.html, CARD_SANITIZE_CONFIG) as unknown as string);
          wrap.setAttribute('dir', 'auto');
          slot.replaceWith(wrap);
        }
      }

      // image occlusion
      const occ = card.occlusion;
      if (occ) {
        const imgs = Array.from(content.querySelectorAll<HTMLImageElement>('img'));
        let img = imgs.find((i) => ioImage && i.getAttribute('src') === ioImage) ?? content.querySelector<HTMLImageElement>('.loci-io img') ?? imgs[0] ?? null;
        if (!img && ioImage) {
          img = document.createElement('img');
          img.src = ioImage;
          img.alt = '';
          content.append(img);
        }
        if (img) {
          img.setAttribute('data-io', '');
          let box = img.parentElement;
          if (!box || !box.classList.contains('loci-io')) {
            box = document.createElement('span');
            box.className = 'loci-io';
            img.replaceWith(box);
            box.append(img);
          }
          let dims = { width: 1, height: 1 };
          if (occlusionUnits(occ.shapes) === 'pixels') {
            await loadImage(img);
            if (cancelled || token !== tokenRef.current) return;
            dims = { width: img.naturalWidth || 1, height: img.naturalHeight || 1 };
          }
          // the image's aspect ratio lets short screens cap the occlusion block by height (45vh)
          const ratio = () => { if (img.naturalWidth && img.naturalHeight) box.style.setProperty('--loci-io-ratio', String(img.naturalWidth / img.naturalHeight)); };
          if (img.complete) ratio(); else img.addEventListener('load', ratio, { once: true });
          const label = side === 'front' ? t('cardFrame.occlusionFront') : t('cardFrame.occlusionBack');
          box.insertAdjacentHTML('beforeend', occlusionSvg(occ, side, dims, { asked: label }) + occlusionLabels(occ, dims));
        }
      }

      renderMath(content);
      applyOneByOne(content, live.current.clozeRevealed);

      if (side === 'back' && animateReveal) {
        void content.offsetWidth; // restart animations
        content.classList.add('is-revealing');
      }

      const oneByOne = new Set(Array.from(content.querySelectorAll('[data-cloze-index]')).map((e) => e.getAttribute('data-cloze-index'))).size;
      live.current.onRendered?.({ side, hasTypeInput: !!inputRef.current || !!slot, hasAudio: sideData.media.length > 0, oneByOne });

      if (autoplay) {
        const names = side === 'front' ? card.front.media : answerAudio(card.front.media, card.back.media, live.current.replayQuestionAudio);
        if (names.length) void playNames(names);
        else stopAudio();
      } else stopAudio();
    })();
    return () => { cancelled = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [card, side]);

  // One-by-one masks follow the parent's count without re-rendering the card.
  useEffect(() => {
    if (contentRef.current) applyOneByOne(contentRef.current, clozeRevealed);
  }, [clozeRevealed]);

  // Anki night-mode classes for imported CSS, following the resolved theme (system included).
  useLayoutEffect(() => {
    for (const el of [pageRef.current, contentRef.current]) {
      if (!el) continue;
      el.classList.toggle('nightMode', theme === 'dark');
      el.classList.toggle('night_mode', theme === 'dark');
    }
  }, [theme]);

  useEffect(() => () => stopAudio(), []);

  useImperativeHandle(ref, () => ({
    replay: () => { const names = audioList(side); if (names.length) void playNames(names); },
    focusTyped: () => { const i = inputRef.current; if (!i) return false; i.focus(); return true; },
    stopAudio,
  }));

  return (
    <>
      <div ref={hostRef} className={cx(s.host, className)} data-side={side} />
      <Lightbox image={zoom} onClose={() => setZoom(null)} />
    </>
  );
}

/** Hides one-by-one clozes at index ≥ revealed; marks the next one. `undefined` reveals all. */
function applyOneByOne(root: HTMLElement, revealed: number | undefined) {
  const spans = Array.from(root.querySelectorAll<HTMLElement>('.cloze[data-cloze-index]'));
  for (const el of spans) {
    const i = Number(el.dataset.clozeIndex ?? 0);
    const hidden = revealed !== undefined && i >= revealed;
    el.classList.toggle('loci-hidden', hidden);
    el.classList.toggle('loci-next', hidden && i === revealed);
    if (hidden) { el.setAttribute('aria-hidden', 'true'); el.title = t('study.revealNext'); }
    else { el.removeAttribute('aria-hidden'); el.removeAttribute('title'); }
  }
}
