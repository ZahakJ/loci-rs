/**
 * Tiny i18n: `t(key, vars)` over JSON dictionaries organised by view.
 *
 * - Dictionaries may be nested (`{"study": {"showAnswer": "…"}}`) or flat with dotted keys;
 *   both are flattened to dotted keys at registration.
 * - `{name}` interpolates vars. ICU-lite: `{count, plural, =0 {none} one {# card} other {# cards}}`
 *   (`#` = the locale-formatted number, exact `=n` branches first, then Intl.PluralRules category,
 *   then `other`) and `{kind, select, a {…} other {…}}`; branches may nest.
 * - Legacy suffix plurals (`key_one` / `key_other` chosen by vars.count) are still honoured.
 * - Missing keys fall back to English, then to the key itself, so the UI never crashes.
 * Formatting helpers use Intl for the current locale.
 */
import en from './en.json';

export type Vars = Record<string, string | number>;
type Dict = Record<string, string>;
type Nested = { [k: string]: string | Nested };

/** Dotted keys of the English dictionary (for editor completion; `t` accepts any string). */
type Leaves<T, P extends string = ''> = {
  [K in keyof T & string]: T[K] extends string ? `${P}${K}` : Leaves<T[K], `${P}${K}.`>;
}[keyof T & string];
export type MessageKey = Leaves<typeof en>;

function flatten(obj: Nested, prefix = '', out: Dict = {}): Dict {
  for (const [k, v] of Object.entries(obj)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (typeof v === 'string') out[key] = v; else if (v && typeof v === 'object') flatten(v, key, out);
  }
  return out;
}

const dictionaries: Record<string, Dict> = { en: flatten(en as Nested) };
let locale = 'en';
let dict: Dict = dictionaries.en!;
let pluralRules = new Intl.PluralRules('en');

/** Languages written right to left. */
const RTL = new Set(['ar', 'arc', 'ckb', 'dv', 'fa', 'ha', 'he', 'iw', 'khw', 'ks', 'ku', 'nqo', 'ps', 'sd', 'syr', 'ug', 'ur', 'yi']);

/** Switches the UI locale; falls back to the base language, then English, for the dictionary. */
export function setLocale(next: string): void {
  const base = next.split('-')[0] ?? 'en';
  locale = next || 'en';
  dict = dictionaries[next] ?? dictionaries[base] ?? dictionaries.en!;
  try { pluralRules = new Intl.PluralRules(locale); } catch { locale = 'en'; pluralRules = new Intl.PluralRules('en'); }
}
export function getLocale(): string { return locale; }

/** Text direction of a locale ('rtl' for Arabic, Hebrew, Persian, Urdu, …). */
export function dir(loc: string = locale): 'ltr' | 'rtl' {
  const lang = (loc.split(/[-_]/)[0] ?? '').toLowerCase();
  if (RTL.has(lang)) return 'rtl';
  // Explicit RTL script subtags, e.g. pa-Arab, az-Arab.
  return /-(?:Arab|Hebr|Syrc|Thaa|Nkoo|Adlm|Rohg)\b/i.test(loc) ? 'rtl' : 'ltr';
}
/** Alias kept for existing callers. */
export const getDir = dir;

/** Adds or extends a locale's dictionary (nested or flat). */
export function registerDictionary(loc: string, d: Nested | Dict): void {
  dictionaries[loc] = { ...(dictionaries[loc] ?? {}), ...flatten(d as Nested) };
  if (loc === locale || loc === locale.split('-')[0]) dict = dictionaries[locale] ?? dictionaries[loc]!;
}
export function hasKey(key: string): boolean { return key in dict || key in dictionaries.en!; }

function lookup(key: string): string | undefined { return dict[key] ?? dictionaries.en![key]; }

/** Index of the `}` matching the `{` at `open`. */
function matching(s: string, open: number): number {
  let depth = 0;
  for (let i = open; i < s.length; i++) {
    if (s[i] === '{') depth++;
    else if (s[i] === '}') { depth--; if (depth === 0) return i; }
  }
  return -1;
}

/** Parses `sel {msg} sel2 {msg2}` into pairs. */
function parseBranches(s: string): Array<[string, string]> {
  const out: Array<[string, string]> = [];
  let i = 0;
  while (i < s.length) {
    while (i < s.length && /\s/.test(s[i]!)) i++;
    const b = s.indexOf('{', i);
    if (b < 0) break;
    const sel = s.slice(i, b).trim();
    const e = matching(s, b);
    if (e < 0) break;
    out.push([sel, s.slice(b + 1, e)]);
    i = e + 1;
  }
  return out;
}

/** Formats an ICU-lite message with vars. `hash` is the value for `#` inside plural branches. */
export function formatMessage(msg: string, vars: Vars = {}, hash?: string): string {
  let out = '';
  let i = 0;
  while (i < msg.length) {
    const ch = msg[i]!;
    if (ch === '#' && hash !== undefined) { out += hash; i++; continue; }
    if (ch !== '{') { out += ch; i++; continue; }
    const end = matching(msg, i);
    if (end < 0) { out += msg.slice(i); break; }
    const body = msg.slice(i + 1, end);
    const c1 = body.indexOf(',');
    if (c1 < 0) {
      const name = body.trim();
      out += name in vars ? (typeof vars[name] === 'number' ? formatNumber(vars[name] as number) : String(vars[name])) : `{${body}}`;
    } else {
      const name = body.slice(0, c1).trim();
      const rest = body.slice(c1 + 1);
      const c2 = rest.indexOf(',');
      const type = (c2 < 0 ? rest : rest.slice(0, c2)).trim();
      const branches = parseBranches(c2 < 0 ? '' : rest.slice(c2 + 1));
      const pick = (sel: string) => branches.find(([s]) => s === sel)?.[1];
      const v = vars[name];
      if (type === 'plural' || type === 'selectordinal') {
        const n = typeof v === 'number' ? v : Number(v);
        const rules = type === 'selectordinal' ? new Intl.PluralRules(locale, { type: 'ordinal' }) : pluralRules;
        const chosen = pick(`=${n}`) ?? pick(Number.isFinite(n) ? rules.select(n) : 'other') ?? pick('other') ?? '';
        out += formatMessage(chosen, vars, Number.isFinite(n) ? formatNumber(n) : String(v ?? ''));
      } else if (type === 'select') {
        const chosen = pick(String(v ?? '')) ?? pick('other') ?? '';
        out += formatMessage(chosen, vars, hash);
      } else if (type === 'number') {
        out += typeof v === 'number' ? formatNumber(v) : String(v ?? '');
      } else {
        out += String(v ?? '');
      }
    }
    i = end + 1;
  }
  return out;
}

/** Translates `key` with optional vars. Unknown keys return the key. */
export function t(key: MessageKey | (string & {}), vars?: Vars): string {
  let s: string | undefined;
  if (vars && typeof vars.count === 'number') {
    s = lookup(`${key}_${pluralRules.select(vars.count)}`) ?? (lookup(`${key}_one`) !== undefined ? lookup(`${key}_other`) : undefined);
  }
  s ??= lookup(key);
  if (s === undefined) return key;
  return vars || s.includes('{') ? formatMessage(s, vars ?? {}) : s;
}

// ───────────────────────── Intl helpers ─────────────────────────

/** Formats a number for the UI locale. */
export function formatNumber(n: number, opts?: Intl.NumberFormatOptions): string {
  return new Intl.NumberFormat(locale, opts).format(n);
}

/** 0.873 → "87%" (fractionDigits default 0). */
export function formatPercent(fraction: number, fractionDigits = 0): string {
  return new Intl.NumberFormat(locale, { style: 'percent', minimumFractionDigits: fractionDigits, maximumFractionDigits: fractionDigits }).format(fraction);
}

/**
 * Formats a duration in seconds: "45 s", "12 min", "1 h 20 min", "3 d 4 h".
 * `style` 'short' (default) or 'long' ("1 hour, 20 minutes"). At most two units.
 */
export function formatDuration(seconds: number, style: 'short' | 'long' | 'narrow' = 'short'): string {
  const s = Math.max(0, Math.round(seconds));
  let parts: Array<[number, 'day' | 'hour' | 'minute' | 'second']>;
  if (s < 60) parts = [[s, 'second']];
  else if (s < 3600) parts = [[Math.round(s / 60), 'minute']];
  else if (s < 86400) { const h = Math.floor(s / 3600); const m = Math.round((s % 3600) / 60); parts = m === 60 ? [[h + 1, 'hour']] : [[h, 'hour'], [m, 'minute']]; }
  else { const d = Math.floor(s / 86400); const h = Math.round((s % 86400) / 3600); parts = h === 24 ? [[d + 1, 'day']] : [[d, 'day'], [h, 'hour']]; }
  parts = parts.filter(([v], i) => v > 0 || i === 0);
  if (parts.length === 1 && parts[0]![1] === 'minute' && parts[0]![0] === 60) parts = [[1, 'hour']];
  const unitDisplay = style === 'long' ? 'long' : style === 'narrow' ? 'narrow' : 'short';
  const pieces = parts.map(([v, unit]) => new Intl.NumberFormat(locale, { style: 'unit', unit, unitDisplay }).format(v));
  if (pieces.length === 1) return pieces[0]!;
  if (style === 'long' && typeof Intl.ListFormat === 'function') return new Intl.ListFormat(locale, { style: 'long', type: 'unit' }).format(pieces);
  return pieces.join(' ');
}

/**
 * Relative day label for an offset from today: 0 → "today", 1 → "tomorrow", -1 → "yesterday",
 * 5 → "in 5 days"; beyond 6 weeks it switches to months, beyond a year to years.
 */
export function formatRelativeDay(daysFromToday: number): string {
  const rtf = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });
  const d = Math.round(daysFromToday);
  const abs = Math.abs(d);
  if (abs < 42) return rtf.format(d, 'day');
  if (abs < 365) return rtf.format(Math.round(d / 30.44), 'month');
  return rtf.format(Math.round(d / 365.25), 'year');
}

/** Locale date, e.g. "6 Oct 2026" (opts override). `when` is ms epoch or a Date. */
export function formatDate(when: number | Date, opts: Intl.DateTimeFormatOptions = { day: 'numeric', month: 'short', year: 'numeric' }): string {
  return new Intl.DateTimeFormat(locale, opts).format(when);
}
