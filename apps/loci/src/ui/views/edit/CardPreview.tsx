/**
 * Live card preview for the editor: renders the note-in-progress through `api.previewNote`
 * (debounced) inside the shared CardFrame. Front/back toggle and, when the note makes several
 * cards (templates or cloze numbers), a card picker.
 */
import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { t } from '@core/i18n/i18n';
import { clozeNumbersInString } from '@core/template/cloze';
import { NotetypeKind, type Notetype, type RenderedCard } from '@core/types';
import { api, errorMessage } from '@ui/api';
import { CardFrame } from '@ui/card/CardFrame';
import { Segmented } from '@ui/design/primitives/Segmented';
import { cx } from '@ui/design/primitives/layer';
import { useTouchOnly } from '@ui/design/primitives/pointer';
import s from './CardPreview.module.css';

export interface CardPreviewProps {
  notetype: Notetype;
  fields: string[];
  className?: string;
  /** heading level / label shown above */
  title?: string;
  /** override the template ordinals to show (note type editor: the template being edited) */
  ord?: number;
  /** extra dependency forcing a re-render (template text being edited) */
  version?: string | number;
  /** replaces the title at the start of the header row */
  headerStart?: ReactNode;
}

/** Card ordinals (0-based) the note would produce, for the picker. */
export function previewOrds(nt: Notetype, fields: string[]): number[] {
  if (nt.kind === NotetypeKind.Cloze) {
    const nums = clozeNumbersInString(fields.join('\n'));
    return nums.length ? nums.map((n) => n - 1) : [];
  }
  return nt.templates.map((tm) => tm.ord);
}

export function CardPreview({ notetype, fields, className, title, ord: forcedOrd, version, headerStart }: CardPreviewProps) {
  const ords = useMemo(() => previewOrds(notetype, fields), [notetype, fields]);
  const [ord, setOrd] = useState(0);
  const [side, setSide] = useState<'front' | 'back'>('front');
  const coarse = useTouchOnly();
  const current = forcedOrd ?? (ords.includes(ord) ? ord : ords[0] ?? 0);
  const key = JSON.stringify(fields);
  const empty = notetype.kind === NotetypeKind.Cloze && ords.length === 0;
  // What is asked for now; the last result remembers which request it answers, so "pending" is
  // derived instead of set from the effect. The effect below runs on every keystroke and must not
  // set state synchronously (that kept React's nested-update counter climbing: E-1).
  const request = `${notetype.id}|${notetype.mtime}|${current}|${version ?? ''}|${key}`;
  const [result, setResult] = useState<{ request: string; card: RenderedCard | null; error: string | null } | null>(null);
  const card = empty ? null : result?.card ?? null;
  const error = empty ? null : result?.error ?? null;
  const pending = !empty && result?.request !== request;

  useEffect(() => {
    if (empty) return;
    let live = true;
    const timer = window.setTimeout(() => {
      api.previewNote(notetype.id, fields, current)
        .then((c) => { if (live) setResult({ request, card: c, error: null }); })
        .catch((e) => { if (live) setResult((r) => ({ request, card: r?.card ?? null, error: errorMessage(e) })); });
    }, 220);
    return () => { live = false; window.clearTimeout(timer); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [request, empty]);

  const cardLabel = (o: number) => (notetype.kind === NotetypeKind.Cloze ? t('editor.previewCloze', { n: o + 1 }) : notetype.templates.find((tm) => tm.ord === o)?.name ?? t('editor.previewCard', { n: o + 1 }));

  return (
    <section className={cx(s.preview, className)} aria-label={title ?? t('editor.preview')}>
      <div className={s.bar}>
        {headerStart ?? (title && <h3 className={s.title}>{title}</h3>)}
        {forcedOrd === undefined && ords.length > 1 && (
          <div className={s.cards} role="group" aria-label={t('editor.previewWhich')}>
            {ords.map((o) => (
              <button key={o} type="button" className={s.cardBtn} aria-pressed={o === current} onClick={() => setOrd(o)}>{cardLabel(o)}</button>
            ))}
          </div>
        )}
        <Segmented
          className={s.sides}
          label={t('editor.previewSide')}
          value={side}
          onChange={setSide}
          options={[{ value: 'front', label: t('editor.front') }, { value: 'back', label: t('editor.back') }]}
        />
      </div>
      <div className={s.stage} data-pending={pending || undefined} aria-live="polite" aria-busy={pending || undefined}>
        {empty ? (
          <p className={s.note}>{t(coarse ? 'editor.noClozesTouch' : 'editor.noClozes')}</p>
        ) : error ? (
          <p className={s.error} role="alert">{error}</p>
        ) : card ? (
          <CardFrame card={card} side={side} className={s.frame} />
        ) : (
          <p className={s.note}>{t('common.loading')}</p>
        )}
      </div>
    </section>
  );
}
