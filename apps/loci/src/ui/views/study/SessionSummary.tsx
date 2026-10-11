import { useEffect, useRef } from 'react';
import { t } from '@core/i18n/i18n';
import type { SessionSummary as Summary } from '@core/types';
import { Icon } from '../../design/icons';
import { Button } from '../../design/primitives/Button';
import { formatPercent } from '../../hooks/format';
import { gradeShares, type GradeKey } from './format';
import s from './Study.module.css';

export interface SessionSummaryProps {
  summary: Summary;
  onHome: () => void;
  onMore: () => void;
}

const ORDER: GradeKey[] = ['again', 'hard', 'good', 'easy'];

/** The quiet end of a session: what you did, how it went, what tomorrow holds. No confetti. */
export function SessionSummary({ summary, onHome, onMore }: SessionSummaryProps) {
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => { heading.current?.focus({ preventScroll: true }); }, []);
  const minutes = Math.max(1, Math.round(summary.seconds / 60));
  const shares = gradeShares(summary);
  const counts: Record<GradeKey, number> = { again: summary.again, hard: summary.hard, good: summary.good, easy: summary.easy };
  return (
    <section className={s.summary} aria-labelledby="study-summary-title">
      <p className={s.eyebrow}>{t('studyScreen.summary.eyebrow')}</p>
      <h1 id="study-summary-title" ref={heading} tabIndex={-1} className={s.summaryTitle}>
        {summary.seconds < 60
          ? t('studyScreen.summary.titleShort', { cards: summary.cards })
          : t('studyScreen.summary.title', { cards: summary.cards, minutes })}
      </h1>

      {summary.cards > 0 && (
        <div className={s.accuracy}>
          <div className={s.accuracyHead}>
            <span className={s.accuracyFigure}>{formatPercent(summary.accuracy)}</span>
            <span className={s.accuracyText}>{t('studyScreen.summary.remembered')}</span>
          </div>
          <div className={s.accuracyBar} role="img" aria-label={t('studyScreen.summary.barAria', { again: summary.again, hard: summary.hard, good: summary.good, easy: summary.easy })}>
            {ORDER.map((k) => shares[k] > 0 && <span key={k} className={s[`seg_${k}`]} style={{ flexGrow: shares[k] }} />)}
          </div>
          <ul role="list" className={s.legend}>
            {ORDER.map((k) => (
              <li key={k}>
                <span className={s[`dot_${k}`]} aria-hidden />
                <span className={s.legendName}>{t(`study.${k}`)}</span>
                <span className="tabular">{counts[k]}</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      <dl className={s.summaryFacts}>
        <div>
          <dt>{t('studyScreen.summary.tomorrow')}</dt>
          <dd>{t('studyScreen.summary.tomorrowValue', { count: summary.tomorrowDue })}</dd>
        </div>
      </dl>

      {summary.insight && (
        <p className={s.summaryInsight}>
          <Icon name="sparkle" size={18} />
          <span>{summary.insight.text}</span>
        </p>
      )}

      <div className={s.summaryActions}>
        <Button variant="primary" size="lg" icon="home" onClick={onHome}>{t('studyScreen.summary.home')}</Button>
        <Button variant="secondary" size="lg" onClick={onMore}>{t('studyScreen.summary.more')}</Button>
      </div>
    </section>
  );
}
