/**
 * WCAG 2.x contrast of the design tokens, both themes (internals §14: "contrast AA in both themes").
 *
 *   cd apps/loci && pnpm contrast        (node --experimental-strip-types scripts/contrast.ts)
 *
 * Parses `src/ui/design/tokens.css` (the `:root` block is light, `:root[data-theme='dark']` is dark),
 * converts every `oklch(L% C H)` to sRGB itself (OKLCH → OKLab → linear sRGB, clipped to gamut), and
 * checks the pairs below. Pill text and background are `color-mix(in oklab, …)` over the theme; the
 * percentages are read from Pill.module.css and mixed in OKLab, as the browser does.
 *
 * Writes the table to docs/design/contrast-0.2.md and exits non-zero when a required pair fails.
 */
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export type Lab = readonly [number, number, number];
export type Rgb = readonly [number, number, number];

/** OKLCH (L 0..1, C, H degrees) to OKLab. */
export function oklchToOklab(l: number, c: number, h: number): Lab {
  const rad = (h * Math.PI) / 180;
  return [l, c * Math.cos(rad), c * Math.sin(rad)];
}

/** OKLab to linear sRGB (unclipped). Matrices from Björn Ottosson's reference implementation. */
export function oklabToLinear([L, a, b]: Lab): Rgb {
  const l_ = L + 0.3963377774 * a + 0.2158037573 * b;
  const m_ = L - 0.1055613458 * a - 0.0638541728 * b;
  const s_ = L - 0.0894841775 * a - 1.291485548 * b;
  const l = l_ ** 3, m = m_ ** 3, s = s_ ** 3;
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ];
}

/** Linear sRGB to OKLab (for hex tokens, so they can take part in colour mixes). */
export function linearToOklab([r, g, b]: Rgb): Lab {
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ];
}

const clip = (x: number) => Math.min(1, Math.max(0, x));
const toGamma = (x: number) => (x <= 0.0031308 ? 12.92 * x : 1.055 * x ** (1 / 2.4) - 0.055);
const toLinear = (x: number) => (x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4);

/** OKLab to gamma-encoded sRGB 0..1, clipped to the gamut. */
export function oklabToSrgb(lab: Lab): Rgb {
  const [r, g, b] = oklabToLinear(lab).map(clip) as unknown as Rgb;
  return [toGamma(r), toGamma(g), toGamma(b)];
}

export function srgbToHex(rgb: Rgb): string {
  return '#' + rgb.map((x) => Math.round(clip(x) * 255).toString(16).padStart(2, '0')).join('');
}

/** `oklch(62.8% 0.2577 29.23)` (or `oklch(0.628 …)`) to hex. */
export function oklchToHex(css: string): string {
  return srgbToHex(oklabToSrgb(parseColor(css)));
}

/** Parses `oklch(L% C H)` or `#rgb/#rrggbb` (or `white`/`black`) into OKLab. */
export function parseColor(css: string): Lab {
  const v = css.trim().toLowerCase();
  const ok = v.match(/^oklch\(\s*([\d.]+)(%?)\s+([\d.]+)\s+([\d.]+)(?:deg)?\s*(?:\/\s*[\d.%]+\s*)?\)$/);
  if (ok) {
    const L = Number(ok[1]) / (ok[2] ? 100 : 1);
    return oklchToOklab(L, Number(ok[3]), Number(ok[4]));
  }
  const named: Record<string, string> = { white: '#ffffff', black: '#000000' };
  const hex = (named[v] ?? v).match(/^#([0-9a-f]{3}|[0-9a-f]{6})$/);
  if (hex) {
    const h = hex[1]!.length === 3 ? hex[1]!.split('').map((c) => c + c).join('') : hex[1]!;
    const rgb = [0, 2, 4].map((i) => toLinear(parseInt(h.slice(i, i + 2), 16) / 255)) as unknown as Rgb;
    return linearToOklab(rgb);
  }
  throw new Error(`unsupported colour: ${css}`);
}

/** `color-mix(in oklab, a p%, b)`. */
export function mixOklab(a: Lab, b: Lab, p: number): Lab {
  return [a[0] * p + b[0] * (1 - p), a[1] * p + b[1] * (1 - p), a[2] * p + b[2] * (1 - p)];
}

/** WCAG relative luminance of an OKLab colour (after gamut clipping). */
export function luminance(lab: Lab): number {
  const [r, g, b] = oklabToSrgb(lab).map(toLinear) as unknown as Rgb;
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

export function contrast(fg: Lab, bg: Lab): number {
  const a = luminance(fg), b = luminance(bg);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

/** Custom properties of the first block whose selector is exactly `selector`. */
export function tokenBlock(css: string, selector: string): Record<string, string> {
  const clean = css.replace(/\/\*[\s\S]*?\*\//g, '');
  const at = clean.indexOf(selector + ' {');
  if (at < 0) throw new Error(`no ${selector} block in tokens.css`);
  const body = clean.slice(clean.indexOf('{', at) + 1, clean.indexOf('}', at));
  const out: Record<string, string> = {};
  for (const m of body.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) out[m[1]!] = m[2]!.trim();
  return out;
}

interface Pair { fg: string; bg: string; min: number; role: string; required: boolean }

const TEXT = 4.5;
const UI = 3.0;

const PAIRS: Pair[] = [
  { fg: 'ink', bg: 'bg', min: TEXT, role: 'body text', required: true },
  { fg: 'ink', bg: 'surface', min: TEXT, role: 'body text on panels', required: true },
  { fg: 'ink-2', bg: 'bg', min: TEXT, role: 'secondary text', required: true },
  { fg: 'muted', bg: 'bg', min: TEXT, role: 'muted text', required: true },
  { fg: 'muted', bg: 'surface', min: TEXT, role: 'muted text on panels', required: true },
  { fg: 'muted', bg: 'surface-2', min: TEXT, role: 'muted text on hover / inset', required: true },
  { fg: 'faint', bg: 'bg', min: UI, role: 'decorative (informational)', required: false },
  { fg: 'accent-strong', bg: 'bg', min: TEXT, role: 'accent text, links', required: true },
  { fg: 'accent-strong', bg: 'surface', min: TEXT, role: 'accent text on panels', required: true },
  { fg: 'on-accent-fill', bg: 'accent-fill', min: TEXT, role: 'primary button label', required: true },
  { fg: 'on-accent-fill', bg: 'accent-fill-hover', min: TEXT, role: 'primary button label, hover', required: true },
  { fg: 'ink', bg: 'accent-soft', min: TEXT, role: 'text on a selection tint', required: true },
  { fg: 'on-accent', bg: 'accent-strong', min: TEXT, role: 'text on accent-strong', required: true },
  { fg: 'danger', bg: 'bg', min: TEXT, role: 'error text', required: true },
  { fg: 'new', bg: 'bg', min: TEXT, role: 'new count (text)', required: true },
  { fg: 'learn', bg: 'bg', min: TEXT, role: 'learning count (text)', required: true },
  { fg: 'due', bg: 'bg', min: TEXT, role: 'due count (text)', required: true },
  { fg: 'again', bg: 'surface', min: UI, role: 'grade rule, legend dot', required: true },
  { fg: 'hard', bg: 'surface', min: UI, role: 'grade rule, legend dot', required: true },
  { fg: 'good', bg: 'surface', min: UI, role: 'grade rule, legend dot', required: true },
  { fg: 'easy', bg: 'surface', min: UI, role: 'grade rule, legend dot', required: true },
];
const PILL_TONES: Array<[string, string]> = [
  ['new', 'new'], ['learn', 'learn'], ['due', 'due'], ['neutral', 'muted'], ['accent', 'accent'],
  ['again', 'again'], ['hard', 'hard'], ['good', 'good'], ['easy', 'easy'],
];

export interface Row { theme: string; pair: string; role: string; ratio: number; min: number; required: boolean; pass: boolean }

/** Every pair in one theme. `pill` holds the Pill.module.css mix percentages (0..1). */
export function evaluate(theme: string, tokens: Record<string, string>, pill: { text: number; bg: number }): Row[] {
  const col = (name: string): Lab => {
    let v = tokens[`--${name}`];
    for (let i = 0; v && v.startsWith('var(') && i < 4; i++) v = tokens[v.slice(4, -1).trim()];
    if (!v) throw new Error(`${theme}: token --${name} missing`);
    return parseColor(v);
  };
  const rows: Row[] = [];
  const push = (pair: string, role: string, fg: Lab, bg: Lab, min: number, required: boolean) => {
    const ratio = contrast(fg, bg);
    rows.push({ theme, pair, role, ratio, min, required, pass: ratio >= min });
  };
  for (const p of PAIRS) push(`${p.fg} / ${p.bg}`, p.role, col(p.fg), col(p.bg), p.min, p.required);
  for (const [name, tone] of PILL_TONES) {
    const fg = mixOklab(col(tone), col('ink'), pill.text);
    const bg = mixOklab(col(tone), col('bg'), pill.bg);
    push(`pill ${name}`, `pill text (${tone} ${Math.round(pill.text * 100)}% ink / ${Math.round(pill.bg * 100)}% bg)`, fg, bg, TEXT, true);
  }
  // 0.2 mixes outside the token table (charts.module.css, ResultsTable.module.css): chart marks are
  // graphical objects (3.0 against the panel); the Browse selection tint must keep ink and muted legible.
  const surface = col('surface');
  push('accent / surface', 'chart series: brass bars and lines', col('accent'), surface, UI, true);
  push('accent-soft / surface', 'chart series: deep brass (accent-strong 72% over surface)', mixOklab(col('accent-strong'), surface, 0.72), surface, UI, true);
  push('slate / surface', 'chart series: slate (ink-2 58% over surface)', mixOklab(col('ink-2'), surface, 0.58), surface, UI, true);
  const selected = mixOklab(col('ink-2'), surface, 0.11);
  push('ink / selection', 'Browse selected row (ink-2 11% over surface)', col('ink'), selected, TEXT, true);
  push('ink-2 / selection', 'Browse selected row, secondary cells', col('ink-2'), selected, TEXT, true);
  push('accent-strong / selection', 'Browse selection rule (2 px)', col('accent-strong'), selected, UI, true);
  return rows;
}

function pillMix(css: string): { text: number; bg: number } {
  const block = css.slice(css.indexOf('.pill {'), css.indexOf('}', css.indexOf('.pill {')));
  const bg = block.match(/background:\s*color-mix\(in oklab,\s*var\(--tone\)\s*([\d.]+)%/);
  const text = block.match(/\scolor:\s*color-mix\(in oklab,\s*var\(--tone\)\s*([\d.]+)%,\s*var\(--ink\)\)/);
  if (!bg || !text) throw new Error('Pill.module.css: .pill colour mixes not found');
  return { text: Number(text[1]) / 100, bg: Number(bg[1]) / 100 };
}

export function table(rows: Row[]): string {
  const lines = ['| Theme | Pair | Role | Ratio | Needs | Result |', '|---|---|---|---:|---:|---|'];
  for (const r of rows) {
    const result = r.pass ? 'PASS' : r.required ? '**FAIL**' : 'below (informational)';
    lines.push(`| ${r.theme} | ${r.pair} | ${r.role} | ${r.ratio.toFixed(2)} | ${r.min.toFixed(1)} | ${result} |`);
  }
  return lines.join('\n');
}

function main(): void {
  const app = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
  const repo = path.dirname(path.dirname(app));
  const css = readFileSync(path.join(app, 'src/ui/design/tokens.css'), 'utf8');
  const pill = pillMix(readFileSync(path.join(app, 'src/ui/design/primitives/Pill.module.css'), 'utf8'));
  const light = tokenBlock(css, ':root');
  const darkOwn = tokenBlock(css, ":root[data-theme='dark']");
  const darkMedia = tokenBlock(css, ":root:not([data-theme='light'])");
  // The OS-dark block must repeat the explicit dark block exactly, or the two dark themes drift.
  const drift = Object.keys({ ...darkOwn, ...darkMedia }).filter((k) => darkOwn[k] !== darkMedia[k]);
  const dark = { ...light, ...darkOwn };
  const rows = [...evaluate('light', light, pill), ...evaluate('dark', dark, pill)];
  const failed = rows.filter((r) => r.required && !r.pass);
  const date = new Date().toISOString().slice(0, 10);
  const md = [
    '# Contrast of the Loci tokens, 0.2',
    '',
    `Generated ${date} by \`apps/loci/scripts/contrast.ts\` (\`pnpm contrast\`) from \`src/ui/design/tokens.css\`.`,
    'Colours are converted from OKLCH to sRGB by the script (OKLab → linear sRGB, clipped to gamut); ratios are WCAG 2.x.',
    '',
    'Thresholds: 4.5 for text (including count numerals and pill labels); 3.0 for large text, UI components and the',
    'grade colours, which appear as 2 px rules and legend dots, never as text. `faint` is decorative and listed for information only.',
    'Pill text is `color-mix(in oklab, tone, ink)` over `color-mix(in oklab, tone, bg)`, with the percentages read from `Pill.module.css`.',
    '',
    table(rows),
    '',
    failed.length ? `**${failed.length} required pair(s) fail.**` : 'Every required pair passes in both themes.',
    '',
  ].join('\n');
  console.log(md);
  // Rewrite only when a ratio changed, so running the gate does not churn the date.
  const out = path.join(repo, 'docs/design/contrast-0.2.md');
  const undated = (x: string) => x.replace(/^Generated \d{4}-\d{2}-\d{2} /m, 'Generated ');
  if (!existsSync(out) || undated(readFileSync(out, 'utf8')) !== undated(md)) writeFileSync(out, md);
  if (drift.length) console.error(`tokens.css: the prefers-color-scheme dark block differs from [data-theme='dark'] in ${drift.join(', ')}`);
  if (failed.length || drift.length) process.exit(1);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main();
