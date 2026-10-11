import { t } from '@core/i18n/i18n';
import type { Stats } from '@core/types';
import { Card } from '../../design/primitives/Card';
import { Heatmap } from '../../design/primitives/Heatmap';
import { Skeleton } from '../../design/primitives/Skeleton';
import { getLocale } from '@core/i18n/i18n';
import s from './Home.module.css';

/** Momentum instead of streaks: nothing resets, a missed day is just an empty square. */
export function MomentumPanel({ stats }: { stats: Stats | undefined }) {
  return (
    <Card as="section" className={s.momentum} aria-labelledby="momentum-title">
      <h2 id="momentum-title" className={s.sectionTitle}>{t('home.momentum')}</h2>
      {!stats ? (
        <div aria-busy="true"><Skeleton width="60%" height="2rem" /><Skeleton height="7rem" radius="md" className="" /></div>
      ) : (
        <>
          <p className={s.momentumFigure}>
            <span className={s.big}>{stats.momentum.daysStudiedLast30}</span>
            <span className={s.momentumText}>{t('home.momentum.days')}</span>
          </p>
          <Heatmap data={stats.heatmap} locale={getLocale()} countLabel={(count) => t('stats.reviewsCount', { count })} summaryLabel={(v) => t('stats.heatmapSummary', v)} lessLabel={t('common.less')} moreLabel={t('common.more')} />
          <p className={s.momentumNote}>
            {t('home.momentum.year', { count: stats.heatmap.reduce((a, d) => a + d.count, 0) })}
          </p>
        </>
      )}
    </Card>
  );
}
