import { t } from '@core/i18n/i18n';
import type { BacklogReport } from '@core/types';
import { Icon } from '../../design/icons';
import { LinkButton } from '../../design/primitives/Button';
import { formatCount, formatMinutes } from '../../hooks/format';
import s from './Home.module.css';

/** Calm, never red: a backlog is a planning problem (DESIGN §9). */
export function CatchUpBanner({ report }: { report: BacklogReport }) {
  return (
    <aside className={s.banner} aria-label={t('home.catchup.label')}>
      <Icon name="calendar" className={s.bannerIcon} />
      <p className={s.bannerText}>
        <strong>{t('home.catchup.waiting', { count: report.due, n: formatCount(report.due) })}</strong>{' '}
        {t('home.catchup.minutes', { time: formatMinutes(report.estimatedMinutes) })}
        {report.maxOverdueDays > 3 ? ` ${t('home.catchup.overdue', { days: report.maxOverdueDays })}` : ''}
      </p>
      <LinkButton href="#/backlog/all" variant="secondary" iconEnd="chevron-right">{t('home.catchup.plan')}</LinkButton>
    </aside>
  );
}
