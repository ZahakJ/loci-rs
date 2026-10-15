/**
 * KaTeX for `.loci-math` spans (internals §2: KaTeX + mhchem). The document stylesheet registers
 * KaTeX's fonts (fonts declared inside a shadow root are ignored by browsers); the same CSS as a
 * string is adopted inside each card's shadow root for the layout rules.
 */
import katex from 'katex';
import 'katex/contrib/mhchem';
import 'katex/dist/katex.min.css';
import katexCss from 'katex/dist/katex.min.css?inline';

let sheet: CSSStyleSheet | null = null;
/** KaTeX's CSS as a constructable stylesheet (without @font-face, which only works in the document). */
export function katexSheet(): CSSStyleSheet {
  if (!sheet) {
    sheet = new CSSStyleSheet();
    sheet.replaceSync(katexCss.replace(/@font-face\s*\{[^}]*\}/g, ''));
  }
  return sheet;
}

/** Renders every `.loci-math` element under `root` in place. Errors show the TeX source quietly. */
export function renderMath(root: ParentNode): void {
  for (const el of Array.from(root.querySelectorAll<HTMLElement>('.loci-math'))) {
    if (el.dataset.rendered) continue;
    const tex = el.textContent ?? '';
    const display = el.dataset.display === '1';
    try {
      katex.render(tex, el, { displayMode: display, throwOnError: false, strict: 'ignore', trust: false, output: 'htmlAndMathml' });
      if (el.querySelector('.katex-error')) { el.textContent = tex; el.dataset.error = '1'; }
    } catch { el.textContent = tex; el.dataset.error = '1'; }
    el.dataset.rendered = '1';
  }
}
