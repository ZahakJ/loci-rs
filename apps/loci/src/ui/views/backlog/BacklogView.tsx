/**
 * Catch-up (DESIGN §9): a backlog is a planning problem. The debt in plain words and minutes,
 * four plans that each say what they do, applied with one tap, undoable.
 */
import { useState, type ReactNode } from 'react';
import { t } from '@core/i18n/i18n';
import type { CatchUpPlan, CatchUpResult } from '@core/types';
import { api, errorMessage, useQuery } from '@ui/api';
import { Icon } from '../../design/icons';
import { Button } from '../../design/primitives/Button';
import { EmptyState } from '../../design/primitives/EmptyState';
import { Pill } from '../../design/primitives/Pill';
import { Segmented } from '../../design/primitives/Segmented';
import { Sheet } from '../../design/primitives/Sheet';
import { Skeleton } from '../../design/primitives/Skeleton';
import { Slider } from '../../design/primitives/Slider';
import { toast } from '../../design/primitives/Toast';
import { cx, useMedia } from '../../design/primitives/layer';
import { formatCount } from '../../hooks/format';
import { navigate } from '../../routes';
import { Page } from '../../shell/Page';
import { LOWER_RETENTION_DAYS, SPREAD_DAYS, debtSentence, paceSentence, planSentence } from './plan';
import s from './Backlog.module.css';

type PlanKind = CatchUpPlan['kind'];

export default function BacklogView({ deckId }: { deckId: number | null }) {
  const report = useQuery((a) => a.backlogReport(deckId), [deckId], ['cards', 'decks']);
  const retention = useQuery(async (a) => {
    const deck = deckId !== null ? await a.getDeck(deckId) : null;
    const preset = await a.getPreset(deck?.configId ?? 1);
    return preset.config.desiredRetention;
  }, [deckId], ['presets']);
  const deck = useQuery((a) => (deckId === null ? Promise.resolve(null) : a.getDeck(deckId)), [deckId], ['decks']);

  const [days, setDays] = useState<(typeof SPREAD_DAYS)[number]>(5);
  const [target, setTarget] = useState<number | null>(null);
  const [applying, setApplying] = useState<PlanKind | null>(null);
  const [result, setResult] = useState<{ plan: CatchUpPlan; result: CatchUpResult } | null>(null);
  const phone = useMedia('(max-width: 719px)');

  const current = retention.data ?? 0.9;
  const lowered = target ?? Math.max(0.75, Math.round((current - 0.05) * 100) / 100);
  const plans: Record<PlanKind, CatchUpPlan> = {
    spread: { kind: 'spread', days },
    'today-share': { kind: 'today-share' },
    'lower-retention': { kind: 'lower-retention', retention: lowered, days: LOWER_RETENTION_DAYS },
    'study-all': { kind: 'study-all' },
  };

  const apply = async (kind: PlanKind) => {
    setApplying(kind);
    try {
      const plan = plans[kind];
      const res = await api.applyCatchUp(deckId, plan);
      setResult({ plan, result: res });
    } catch (e) {
      toast({ message: t('catchUpView.failed', { error: errorMessage(e) }), tone: 'danger' });
    } finally {
      setApplying(null);
    }
  };

  const undo = async () => {
    try {
      const res = await api.undo();
      setResult(null);
      toast({ message: res ? t('catchUpView.undone') : t('undo.nothing'), icon: 'undo' });
    } catch {
      toast({ message: t('undo.failed'), tone: 'danger' });
    }
  };

  const study = () => navigate(`#/study/${deckId ?? 'all'}`);
  const r = report.data;
  const deckLabel = deckId === null ? null : deck.data?.name.split('::').pop();

  return (
    <Page
      title={t('catchUp.title')}
      eyebrow={deckLabel ?? undefined}
      back={{ href: '#/', label: t('catchUpView.home') }}
      width="default"
    >
      {report.loading && !r ? (
        <div className={s.loading} aria-busy="true">
          <Skeleton height="2.5rem" width="80%" />
          <Skeleton height="1rem" width="55%" />
          <div className={s.plans}>{[0, 1, 2, 3].map((i) => <Skeleton key={i} height="13rem" radius="lg" />)}</div>
        </div>
      ) : report.error && !r ? (
        <EmptyState icon="warning" title={t('catchUpView.loadFailed')} body={errorMessage(report.error)} actions={<Button onClick={() => void report.refetch()}>{t('common.retry')}</Button>} />
      ) : r && r.due === 0 ? (
        <EmptyState
          icon="check"
          title={t('catchUpView.clearTitle')}
          body={t('catchUpView.clearBody')}
          actions={<Button variant="primary" icon="home" onClick={() => navigate('#/')}>{t('catchUpView.home')}</Button>}
        />
      ) : r ? (
        <div className={s.view}>
          <section className={s.debt} aria-labelledby="debt-sentence">
            <p id="debt-sentence" className={s.debtSentence}>{debtSentence(r)}</p>
            <p className={s.pace}>{paceSentence(r)} {r.triggered ? t('catchUp.debtCalm') : t('catchUpView.notBehind')}</p>
            <p className={s.why}>
              <Icon name="info" size={18} />
              <span>{t('catchUp.fsrsNote')} {t('catchUpView.whyMore')}</span>
            </p>
          </section>

          <section aria-labelledby="plans-title">
            <h2 id="plans-title" className={s.plansTitle}>{t('catchUpView.choose')}</h2>
            <ul role="list" className={s.plans}>
              <PlanCard
                title={t('catchUp.spread', { days })}
                recommended
                sentence={planSentence(plans.spread, r, current)}
                busy={applying === 'spread'}
                disabled={applying !== null}
                onChoose={() => void apply('spread')}
                control={(
                  <Segmented
                    label={t('catchUpView.daysLabel')}
                    value={String(days)}
                    onChange={(v) => setDays(Number(v) as (typeof SPREAD_DAYS)[number])}
                    options={SPREAD_DAYS.map((d) => ({ value: String(d), label: t('catchUpView.daysShort', { count: d }), ariaLabel: t('common.days', { count: d }) }))}
                    block
                  />
                )}
              />
              <PlanCard
                title={t('catchUp.todayShare')}
                sentence={planSentence(plans['today-share'], r, current)}
                busy={applying === 'today-share'}
                disabled={applying !== null}
                onChoose={() => void apply('today-share')}
              />
              <PlanCard
                title={t('catchUp.lowerRetention')}
                sentence={planSentence(plans['lower-retention'], r, current)}
                figures={[`${Math.round(lowered * 100)}%`, `${Math.round(current * 100)}%`]}
                busy={applying === 'lower-retention'}
                disabled={applying !== null}
                onChoose={() => void apply('lower-retention')}
                control={(
                  <Slider
                    label={t('catchUpView.retentionLabel')}
                    value={lowered}
                    onChange={setTarget}
                    min={0.75}
                    max={Math.max(0.76, Math.round((current - 0.01) * 100) / 100)}
                    step={0.01}
                    format={(v) => `${Math.round(v * 100)}%`}
                    showScale
                  />
                )}
              />
              <PlanCard
                title={t('catchUp.studyAll')}
                sentence={planSentence(plans['study-all'], r, current)}
                busy={applying === 'study-all'}
                disabled={applying !== null}
                onChoose={() => void apply('study-all')}
              />
            </ul>
          </section>
        </div>
      ) : null}

      <Sheet
        open={result !== null}
        onClose={() => setResult(null)}
        title={t('catchUpView.resultTitle')}
        width="28rem"
        phone={phone ? 'bottom' : 'full'}
        footer={(
          <>
            <Button variant="ghost" icon="undo" onClick={() => void undo()}>{t('common.undo')}</Button>
            <Button variant="primary" icon="play" onClick={study} data-autofocus>{t('catchUpView.start')}</Button>
          </>
        )}
      >
        {result && (
          <div className={s.result}>
            <div className={s.resultFigures}>
              <div className={s.figure}>
                <span className={cx(s.figureValue, 'num-lg')}>{formatCount(result.result.keptToday)}</span>
                <span className={s.figureLabel}>{t('catchUpView.keptToday')}</span>
              </div>
              <div className={s.figure}>
                <span className={cx(s.figureValue, 'num-lg')}>{formatCount(result.result.postponed)}</span>
                <span className={s.figureLabel}>{t('catchUpView.postponed')}</span>
              </div>
            </div>
            <p className={s.resultText} role="status">{t('catchUp.applied', { postponed: result.result.postponed, kept: result.result.keptToday })}</p>
            <p className={s.resultNote}>{t('catchUpView.resultNote')}</p>
          </div>
        )}
      </Sheet>
    </Page>
  );
}

/** The sentence with its key figures in <strong>, as Settings writes the retention sentence. */
function withFigures(text: string, figures: string[] = []): ReactNode {
  const out: ReactNode[] = [];
  let rest = text;
  for (const f of figures) {
    const at = rest.indexOf(f);
    if (at < 0) continue;
    out.push(rest.slice(0, at), <strong key={out.length}>{f}</strong>);
    rest = rest.slice(at + f.length);
  }
  out.push(rest);
  return out;
}

function PlanCard({ title, sentence, figures, control, recommended, busy, disabled, onChoose }: {
  title: string; sentence: string; figures?: string[]; control?: ReactNode; recommended?: boolean; busy: boolean; disabled: boolean; onChoose: () => void;
}) {
  return (
    <li className={cx(s.plan, recommended && s.recommended)}>
      <div className={s.planHead}>
        <h3 className={s.planTitle}>{title}</h3>
        {recommended && <Pill tone="accent">{t('common.recommended')}</Pill>}
      </div>
      {control && <div className={s.planControl}>{control}</div>}
      <p className={s.planSentence} aria-live="polite">{withFigures(sentence, figures)}</p>
      <div className={s.planFoot}>
        <Button variant={recommended ? 'primary' : 'secondary'} onClick={onChoose} loading={busy} disabled={disabled && !busy} block>
          {t('catchUpView.choosePlan')}
        </Button>
      </div>
    </li>
  );
}

