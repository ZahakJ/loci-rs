/** Pure helpers for template editing (tested): field references and insertion. */

const esc = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/**
 * Renames `{{Field}}`, `{{#Field}}`, `{{^Field}}`, `{{/Field}}` and filtered forms such as
 * `{{type:Field}}`, `{{cloze:Field}}`, `{{text:hint:Field}}` (Anki does the same on rename).
 */
export function renameFieldRefs(template: string, from: string, to: string): string {
  if (!from || from === to) return template;
  const re = new RegExp(`\\{\\{([#^/]?)((?:[^{}:]+:)*)\\s*${esc(from)}\\s*\\}\\}`, 'g');
  return template.replace(re, (_m, sigil: string, filters: string) => `{{${sigil}${filters}${to}}}`);
}

/** Field names referenced by a template (with filters and section markers stripped). */
export function referencedFields(template: string): string[] {
  const out = new Set<string>();
  for (const m of template.matchAll(/\{\{([#^/]?)([^{}]+)\}\}/g)) {
    const name = m[2]!.split(':').pop()!.trim();
    if (name) out.add(name);
  }
  return [...out];
}

/** A unique name: `base`, `base 2`, `base 3`… */
export function uniqueName(base: string, taken: string[]): string {
  const set = new Set(taken.map((x) => x.toLowerCase()));
  if (!set.has(base.toLowerCase())) return base;
  for (let i = 2; ; i++) { const n = `${base} ${i}`; if (!set.has(n.toLowerCase())) return n; }
}

/** Moves an item from one index to another (returns a new array). */
export function moveItem<T>(arr: T[], from: number, to: number): T[] {
  if (from === to || from < 0 || to < 0 || from >= arr.length || to >= arr.length) return arr;
  const next = [...arr];
  const [x] = next.splice(from, 1);
  next.splice(to, 0, x!);
  return next;
}
