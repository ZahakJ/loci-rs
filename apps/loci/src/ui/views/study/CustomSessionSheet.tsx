import { useEffect, useId, useState } from 'react';
import { t } from '@core/i18n/i18n';
import { EXTEND_TODAY, errorMessage, extendTodayLimits, useApiHas } from '../../api';
import { Button } from '../../design/primitives/Button';
import { Field } from '../../design/primitives/Field';
import { Input } from '../../design/primitives/Input';
import { Segmented } from '../../design/primitives/Segmented';
import { Sheet } from '../../design/primitives/Sheet';
import { toast } from '../../design/primitives/Toast';
import { cx } from '../../design/primitives/layer';
import { navigate, formatRoute } from '../../routes';
import { CUSTOM_SEARCH_LIMIT, formatCustom, type CustomKind } from './format';
import s from './Study.module.css';
import { Radio } from '../../design/primitives/Radio';

export interface CustomSessionSheetProps {
  open: boolean;
  onClose: () => void;
  deckId: number | null;
  /** preselected kind (e.g. 'ahead' from the empty state) */
  initial?: SheetKind;
  /** called instead of navigating when the route would not change (restart in place) */
  onRestart?: (custom: string) => void;
}

/** Session kinds, plus 'extend' (raise today's limits, then study as usual). */
export type SheetKind = CustomKind['kind'] | 'extend';

const AHEAD = ['1', '3', '7', '14'] as const;
const EXTRA_NEW = ['0', '5', '10', '20', '50'] as const;
const EXTRA_REVIEW = ['0', '50', '100', '200'] as const;
const FORGOTTEN = ['1', '7', '30'] as const;
const LIMITS = ['50', '100', '200'] as const;

/** Custom session (DESIGN §3: covers the common uses of filtered decks). */
export function CustomSessionSheet({ open, onClose, deckId, initial = 'ahead', onRestart }: CustomSessionSheetProps) {
  const [kind, setKind] = useState<SheetKind>(initial);
  const [extraNew, setExtraNew] = useState<(typeof EXTRA_NEW)[number]>('10');
  const [extraReview, setExtraReview] = useState<(typeof EXTRA_REVIEW)[number]>('0');
  const [busy, setBusy] = useState(false);
  // 'Extend today's limits' needs a collection that supports it; hidden otherwise.
  const canExtend = useApiHas(EXTEND_TODAY);
  const [ahead, setAhead] = useState<(typeof AHEAD)[number]>('3');
  const [forgotten, setForgotten] = useState<(typeof FORGOTTEN)[number]>('7');
  const [query, setQuery] = useState('');
  const [limit, setLimit] = useState<(typeof LIMITS)[number]>(String(CUSTOM_SEARCH_LIMIT) as '100');
  const name = useId();
  useEffect(() => { if (open) { setKind(initial === 'extend' && !canExtend ? 'ahead' : initial); setBusy(false); } }, [open, initial, canExtend]);

  const build = (): CustomKind | null => {
    switch (kind) {
      case 'extend': return null;
      case 'ahead': return { kind, days: Number(ahead) };
      case 'forgotten': return { kind, days: Number(forgotten) };
      case 'preview': return { kind };
      case 'search': return query.trim() ? { kind, query: query.trim(), limit: Number(limit) } : null;
    }
  };
  const chosen = build();
  const extendReady = kind === 'extend' && canExtend && (Number(extraNew) > 0 || Number(extraReview) > 0);

  const go = (custom: string | undefined) => {
    const hash = formatRoute(custom ? { name: 'study', deckId, custom } : { name: 'study', deckId });
    onClose();
    if (window.location.hash === hash && onRestart) onRestart(custom ?? '');
    else navigate(hash);
  };
  const start = async () => {
    if (kind === 'extend') {
      if (!extendReady || busy) return;
      setBusy(true);
      try {
        await extendTodayLimits(deckId, Number(extraNew), Number(extraReview));
        go(undefined);
      } catch (e) {
        toast({ message: t('studyScreen.custom.extendFailed', { error: errorMessage(e) }), tone: 'danger' });
      } finally {
        setBusy(false);
      }
      return;
    }
    if (chosen) go(formatCustom(chosen));
  };

  const options: Array<{ value: SheetKind; title: string; help: string }> = [
    ...(canExtend ? [{ value: 'extend' as const, title: t('customStudy.extend'), help: deckId === null ? t('studyScreen.custom.extendHelpAll') : t('studyScreen.custom.extendHelp') }] : []),
    { value: 'ahead', title: t('customStudy.ahead'), help: t('studyScreen.custom.aheadHelp') },
    { value: 'forgotten', title: t('customStudy.forgotten'), help: t('studyScreen.custom.forgottenHelp') },
    { value: 'preview', title: t('customStudy.preview'), help: t('studyScreen.custom.previewHelp') },
    { value: 'search', title: t('customStudy.search'), help: t('studyScreen.custom.searchHelp') },
  ];

  return (
    <Sheet
      open={open}
      onClose={onClose}
      title={t('customStudy.title')}
      width="30rem"
      phone="bottom"
      footer={(
        <>
          <Button variant="ghost" onClick={onClose}>{t('common.cancel')}</Button>
          <Button variant="primary" icon="play" onClick={() => void start()} loading={busy} disabled={kind === 'extend' ? !extendReady : !chosen}>{t('customStudy.start')}</Button>
        </>
      )}
    >
      <form className={s.custom} onSubmit={(e) => { e.preventDefault(); void start(); }}>
        <p className={s.customLede}>{t('studyScreen.custom.lede')}</p>
        <fieldset className={s.customSet}>
          <legend className="sr-only">{t('customStudy.title')}</legend>
          {options.map((o) => (
            <div key={o.value} className={cx(s.customOption, kind === o.value && s.customChosen)}>
              <Radio name={name} value={o.value} checked={kind === o.value} onChange={() => setKind(o.value)} label={o.title} description={o.help} className={s.customLabel} />
              {kind === o.value && o.value === 'ahead' && (
                <div className={s.customControl}>
                  <span className={s.customControlLabel} id={`${name}-ahead`}>{t('customStudy.aheadDays')}</span>
                  <Segmented label={t('customStudy.aheadDays')} value={ahead} onChange={setAhead} options={AHEAD.map((d) => ({ value: d, label: t('common.days', { count: Number(d) }) }))} />
                </div>
              )}
              {kind === o.value && o.value === 'extend' && (
                <div className={s.customControl}>
                  <span className={s.customControlLabel}>{t('studyScreen.custom.extraNew')}</span>
                  <Segmented label={t('studyScreen.custom.extraNew')} value={extraNew} onChange={setExtraNew} options={EXTRA_NEW.map((n) => ({ value: n, label: n === '0' ? '0' : t('customStudy.plus', { count: Number(n) }) }))} />
                  <span className={s.customControlLabel}>{t('studyScreen.custom.extraReview')}</span>
                  <Segmented label={t('studyScreen.custom.extraReview')} value={extraReview} onChange={setExtraReview} options={EXTRA_REVIEW.map((n) => ({ value: n, label: n === '0' ? '0' : t('customStudy.plus', { count: Number(n) }) }))} />
                </div>
              )}
              {kind === o.value && o.value === 'forgotten' && (
                <div className={s.customControl}>
                  <span className={s.customControlLabel}>{t('customStudy.forgottenDays')}</span>
                  <Segmented label={t('customStudy.forgottenDays')} value={forgotten} onChange={setForgotten} options={FORGOTTEN.map((d) => ({ value: d, label: t('common.days', { count: Number(d) }) }))} />
                </div>
              )}
              {kind === o.value && o.value === 'search' && (
                <div className={s.customControl}>
                  <Field label={t('studyScreen.custom.query')} help={t('studyScreen.custom.queryHelp')}>
                    <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="tag:verb -is:new" mono spellCheck={false} autoCapitalize="off" autoComplete="off" />
                  </Field>
                  <span className={s.customControlLabel}>{t('studyScreen.custom.limit')}</span>
                  <Segmented label={t('studyScreen.custom.limit')} value={limit} onChange={setLimit} options={LIMITS.map((l) => ({ value: l, label: t('customStudy.limit', { count: Number(l) }) }))} />
                </div>
              )}
            </div>
          ))}
        </fieldset>
        <button type="submit" hidden />
      </form>
    </Sheet>
  );
}
