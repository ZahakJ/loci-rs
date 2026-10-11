/** Pure helpers for the Import view (tested). */
import { t } from '@core/i18n/i18n';
import type { ImportOptions, ImportPreview } from '@core/types';
import { formatCount } from '../../hooks/format';

/** Fields newer collections add to the preview (how many notes are already here); optional. */
export type PreviewCounts = ImportPreview & { notesExisting?: number; notesNew?: number };

/** A Loci full backup (.zip): it can only replace the whole collection. */
export function isLociBackupFile(name: string): boolean { return /\.zip$/i.test(name); }

/** Each warning once (identical texts and repeated filtered-deck notes collapse), minus ones already shown. */
export function uniqueWarnings(list: string[], alreadyShown: string[] = []): string[] {
  const out: string[] = [];
  let filtered = alreadyShown.some((w) => /filtered deck/i.test(w));
  for (const w of list) {
    if (out.includes(w) || alreadyShown.includes(w)) continue;
    if (/filtered deck/i.test(w)) { if (filtered) continue; filtered = true; }
    out.push(w);
  }
  return out;
}

/** The Import button: 'Import 12 new notes', 'Import 40 notes', or null when nothing is new. */
export function importButtonLabel(p: PreviewCounts, mode: ImportOptions['mode']): string | null {
  if (mode === 'replace' || typeof p.notesNew !== 'number') return t('import.go', { n: formatCount(p.notes) });
  if (p.notesNew <= 0) return null;
  return t('import.goNew', { count: p.notesNew, n: formatCount(p.notesNew) });
}
