/**
 * Card info: "why this interval?" (DESIGN §8). The plain sentence first, then the numbers,
 * then the review history. Used as a side panel / sheet from Study and reusable from Browse.
 */
import { getLocale, t } from '@core/i18n/i18n';
import { formatInterval } from '@core/scheduler/convert';
import { RevlogType, type CardExplanation, type RevlogEntry } from '@core/types';
import { useQuery } from '@ui/api';
import { Sheet } from '../../design/primitives/Sheet';
import { Skeleton } from '../../design/primitives/Skeleton';
import { EmptyState } from '../../design/primitives/EmptyState';
import { cx, useMedia } from '../../design/primitives/layer';
import { formatPercent, formatRelative } from '../../hooks/format';
import s from './CardInfo.module.css';

export interface CardInfoPanelProps {
  cardId: number;
  className?: string;
  /** heading level for the section titles (default 3) */
  headingLevel?: 2 | 3;
}

const GRADE = ['manual', 'again', 'hard', 'good', 'easy'] as const;

/** Interval of a revlog row: positive = days, negative = seconds (Anki). */
export function revlogInterval(ivl: number): string {
  if (ivl === 0) return '–';
  return formatInterval(ivl > 0 ? ivl * 86400 : -ivl);
}

function revlogKind(e: RevlogEntry): string {
  switch (e.type) {
    case RevlogType.Learn: return t('cardInfoPanel.type.learn');
    case RevlogType.Review: return t('cardInfoPanel.type.review');
    case RevlogType.Relearn: return t('cardInfoPanel.type.relearn');
    case RevlogType.Filtered: return t('cardInfoPanel.type.filtered');
    case RevlogType.Manual: return t('cardInfoPanel.type.manual');
    case RevlogType.Rescheduled: return t('cardInfoPanel.type.rescheduled');
    default: return '';
  }
}

function shortDate(ms: number): string {
  const d = new Date(ms);
  const sameYear = d.getFullYear() === new Date().getFullYear();
  return new Intl.DateTimeFormat(getLocale(), { day: 'numeric', month: 'short', ...(sameYear ? {} : { year: 'numeric' }) }).format(d);
}

function days(n: number): string {
  if (n < 1) return t('cardInfoPanel.lessThanDay');
  const r = n < 10 ? Math.round(n * 10) / 10 : Math.round(n);
  return t('cardInfoPanel.days', { count: r });
}

function Ring({ value }: { value: number | null }) {
  const r = 26;
  const c = 2 * Math.PI * r;
  const v = value === null ? 0 : Math.max(0, Math.min(1, value));
  // brass, whatever the value: the number says how likely, the colour does not judge it
  return (
    <div className={s.ring}>
      <svg viewBox="0 0 64 64" width="72" height="72" aria-hidden>
        <circle cx="32" cy="32" r={r} className={s.ringTrack} />
        {value !== null && (
          <circle cx="32" cy="32" r={r} className={s.ringArc} strokeDasharray={`${c * v} ${c}`} transform="rotate(-90 32 32)" />
        )}
      </svg>
      <span className={cx(s.ringValue, 'num-lg')}>{value === null ? '–' : formatPercent(v)}</span>
    </div>
  );
}

export function CardInfoContent({ info, headingLevel = 3 }: { info: CardExplanation; headingLevel?: 2 | 3 }) {
  const H = headingLevel === 2 ? 'h2' : 'h3';
  const now = Date.now() / 1000;
  const rows: Array<[string, string]> = [];
  if (info.stability !== null) rows.push([t('cardInfoPanel.stability'), days(info.stability)]);
  if (info.difficulty !== null) rows.push([t('cardInfoPanel.difficulty'), t('cardInfoPanel.difficultyValue', { value: (Math.round(info.difficulty * 10) / 10).toLocaleString(getLocale()) })]);
  rows.push([t('cardInfoPanel.due'), info.dueDate !== null ? `${shortDate(info.dueDate * 1000)} · ${formatRelative(info.dueDate, now)}` : t('cardInfoPanel.notScheduled')]);
  rows.push([t('cardInfoPanel.interval'), info.interval > 0 ? days(info.interval) : '–']);
  rows.push([t('cardInfoPanel.reviews'), info.reps.toLocaleString(getLocale())]);
  rows.push([t('cardInfoPanel.lapses'), info.lapses.toLocaleString(getLocale())]);
  if (info.averageSeconds > 0) rows.push([t('cardInfoPanel.average'), t('cardInfoPanel.seconds', { value: (Math.round(info.averageSeconds * 10) / 10).toLocaleString(getLocale()) })]);

  return (
    <div className={s.info}>
      <p className={s.sentence}>{info.sentence}</p>

      <div className={s.recall}>
        <Ring value={info.retrievability} />
        <div className={s.recallText}>
          <span className={s.recallLabel}>{t('cardInfoPanel.recallNow')}</span>
          <span className={s.recallHelp}>
            {info.retrievability === null ? t('cardInfoPanel.recallUnknown') : t('cardInfoPanel.recallHelp', { percent: formatPercent(info.retrievability) })}
          </span>
        </div>
      </div>

      <dl className={s.facts}>
        {rows.map(([k, v]) => (
          <div key={k} className={s.fact}>
            <dt>{k}</dt>
            <dd><span className="num">{v}</span></dd>
          </div>
        ))}
      </dl>

      <section className={s.history} aria-labelledby="cardinfo-history">
        <H id="cardinfo-history" className={s.historyTitle}>{t('cardInfoPanel.history')}</H>
        {info.history.length === 0 ? (
          <p className={s.historyEmpty}>{t('cardInfoPanel.noHistory')}</p>
        ) : (
          <ol role="list" className={s.historyList}>
            {info.history.map((e) => {
              const g = GRADE[Math.max(0, Math.min(4, e.ease))]!;
              return (
                <li key={e.id} className={s.historyRow}>
                  <time className={cx(s.historyDate, 'num')} dateTime={new Date(e.id).toISOString()}>{shortDate(e.id)}</time>
                  <span className={s.historyGrade}>
                    <span className={cx(s.gradeDot, s[g])} aria-hidden />
                    <span>{g === 'manual' ? t('cardInfoPanel.type.manual') : t(`study.${g}`)}</span>
                  </span>
                  <span className={s.historyKind}>{revlogKind(e)}</span>
                  <span className={cx(s.historyIvl, 'num')}><span className="sr-only">{t('cardInfoPanel.interval')}: </span>{revlogInterval(e.ivl)}</span>
                </li>
              );
            })}
          </ol>
        )}
      </section>
    </div>
  );
}

/** Loads and shows the explanation for one card. */
export function CardInfoPanel({ cardId, className, headingLevel }: CardInfoPanelProps) {
  const q = useQuery((a) => a.explainCard(cardId), [cardId], ['cards']);
  return (
    <div className={cx(s.panel, className)} aria-busy={q.loading && !q.data ? true : undefined}>
      {q.data ? <CardInfoContent info={q.data} headingLevel={headingLevel} />
        : q.error ? <EmptyState compact icon="warning" title={t('cardInfoPanel.error')} />
          : (
            <div className={s.loading}>
              <Skeleton height="3.2em" />
              <Skeleton height="72px" width="60%" />
              <Skeleton height="10rem" />
            </div>
          )}
    </div>
  );
}

/** Card info as a side sheet on desktop and a bottom sheet on phones. */
export function CardInfoSheet({ open, onClose, cardId }: { open: boolean; onClose: () => void; cardId: number | null }) {
  const phone = useMedia('(max-width: 719px)');
  return (
    <Sheet open={open && cardId !== null} onClose={onClose} title={t('study.whyInterval')} width="26rem" phone={phone ? 'bottom' : 'full'}>
      {cardId !== null && <CardInfoPanel cardId={cardId} />}
    </Sheet>
  );
}
