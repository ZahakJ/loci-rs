import type { CSSProperties } from 'react';
import { t } from '@core/i18n/i18n';
import type { Insight } from '@core/types';
import { Icon } from '../../design/icons';
import { Card } from '../../design/primitives/Card';
import { cx } from '../../design/primitives/layer';
import s from './Home.module.css';

const ICON = { suggest: 'sparkle', warn: 'warning', info: 'info' } as const;

export function InsightsStrip({ insights }: { insights: Insight[] }) {
  if (insights.length === 0) return null;
  return (
    <section aria-labelledby="insights-title">
      <div className={s.sectionHead}>
        <h2 id="insights-title" className={s.sectionTitle}>{t('home.insights')}</h2>
      </div>
      <ul role="list" className={cx(s.insights, 'stagger')}>
        {insights.slice(0, 3).map((ins, i) => (
          <Card as="li" key={ins.id} padding="none" className={cx(s.insight, s[ins.severity])} style={{ '--i': i } as CSSProperties}>
            <div className={s.insightTop}>
              <span className={s.insightIcon} aria-hidden><Icon name={ICON[ins.severity]} size={16} /></span>
              <p className={s.insightText}>{ins.text}</p>
            </div>
            {ins.action && (
              <a className={s.insightAction} href={ins.action.route}>
                {ins.action.label}<Icon name="chevron-right" size={14} />
              </a>
            )}
          </Card>
        ))}
      </ul>
    </section>
  );
}
