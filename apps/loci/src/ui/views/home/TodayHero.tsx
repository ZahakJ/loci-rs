import { t } from '@core/i18n/i18n';
import type { DeckTreeNode } from '@core/types';
import { Button, LinkButton } from '../../design/primitives/Button';
import { Pill } from '../../design/primitives/Pill';
import { Skeleton } from '../../design/primitives/Skeleton';
import { formatMinutes, formatToday } from '../../hooks/format';
import { navigate } from '../../routes';
import { budgetLine } from './budget';
import s from './Home.module.css';

export interface TodayHeroProps {
  tree: DeckTreeNode | undefined;
  /** seconds studied today (stats.today.seconds) and the daily time budget in minutes */
  studiedSeconds?: number;
  budgetMinutes?: number;
  onCustom: () => void;
}

export function TodayHero({ tree, studiedSeconds, budgetMinutes, onCustom }: TodayHeroProps) {
  if (!tree) {
    return (
      <section className={s.hero} aria-busy="true" aria-label={t('home.today')}>
        <Skeleton width="10rem" />
        <Skeleton width="min(32rem, 90%)" height="2.5rem" />
        <Skeleton width="16rem" height="1.5rem" />
        <Skeleton width="10rem" height="3rem" radius="md" />
      </section>
    );
  }
  const total = tree.newCount + tree.learnCount + tree.reviewCount;
  const minutes = tree.estimatedMinutes;
  const budget = budgetLine(studiedSeconds, budgetMinutes);
  const budgetNode = budget && (
    <p className={s.budget} data-budget-line>
      <span className={s.budgetBar} aria-hidden><span style={{ transform: `scaleX(${Math.min(1, (studiedSeconds ?? 0) / 60 / (budgetMinutes || 1))})` }} /></span>
      {budget}
    </p>
  );
  return (
    <section className={s.hero} aria-labelledby="today-title">
      <div className={s.date}>{formatToday()}</div>
      {total > 0 ? (
        <>
          <h2 id="today-title" className={s.sentence}>
            {t('home.hero.before')}<em>{formatMinutes(minutes)}</em>{t('home.hero.after')}
          </h2>
          {budgetNode}
          <div className={s.counts} aria-label={t('home.countsLabel')}>
            <Pill tone="new" dot size="lg" value={tree.newCount} label={t('counts.new', { count: tree.newCount })}>{' '}{t('counts.newShort')}</Pill>
            <Pill tone="learn" dot size="lg" value={tree.learnCount} label={t('counts.learn', { count: tree.learnCount })}>{' '}{t('counts.learnShort')}</Pill>
            <Pill tone="due" dot size="lg" value={tree.reviewCount} label={t('counts.due', { count: tree.reviewCount })}>{' '}{t('counts.dueShort')}</Pill>
          </div>
          <div className={s.heroActions}>
            {/* Start is focused on arrival so Enter begins the session (DESIGN §18). */}
            <Button variant="primary" size="lg" kbd="enter" className={s.startButton} autoFocus onClick={() => navigate('#/study/all')}>
              {t('home.start')}
            </Button>
            <Button variant="ghost" size="lg" icon="calendar" onClick={onCustom} aria-haspopup="dialog">{t('home.customSession')}</Button>
          </div>
        </>
      ) : (
        <>
          <h2 id="today-title" className={s.sentence}>{t('home.hero.done')}</h2>
          {budgetNode}
          <p className={s.date}>{t('home.hero.doneSub')}</p>
          <div className={s.heroActions}>
            <LinkButton variant="secondary" size="lg" href="#/study/all?custom=ahead%3A1" icon="calendar" autoFocus>{t('home.studyAhead')}</LinkButton>
            <LinkButton variant="ghost" size="lg" href="#/study/all?custom=preview" icon="play">{t('home.previewNew')}</LinkButton>
            <Button variant="ghost" size="lg" icon="settings" onClick={onCustom} aria-haspopup="dialog">{t('home.customSession')}</Button>
          </div>
        </>
      )}
    </section>
  );
}
