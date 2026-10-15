/** Pure helpers for the sync UI (no engine access, so they are unit-tested). */
import { t } from '@core/i18n/i18n';
import { localDateStamp } from '../hooks/format';

/** File name for a hand-off sync file. */
export function syncFileName(d: Date = new Date()): string {
  return `loci-${localDateStamp(d)}.loci-sync`;
}

/** One line about what a sync brought in. */
export function describeSync(r: { rowsIn: number; reviewsAdded: number; mediaIn?: number; mediaAdded?: number; complete?: boolean; incomplete?: boolean }): string {
  const media = r.mediaIn ?? r.mediaAdded ?? 0;
  const parts: string[] = [];
  if (r.rowsIn === 0 && media === 0) parts.push(t('sync.upToDate'));
  else {
    parts.push(t('sync.changes', { count: r.rowsIn, n: r.rowsIn }));
    if (r.reviewsAdded > 0) parts.push(t('sync.reviews', { count: r.reviewsAdded, n: r.reviewsAdded }));
    if (media > 0) parts.push(t('sync.files', { count: media, n: media }));
  }
  if (r.complete === false || r.incomplete) parts.push(t('sync.again'));
  return parts.join(' · ');
}
