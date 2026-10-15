import { t } from '@core/i18n/i18n';
import { Button, LinkButton } from '../design/primitives/Button';
import { EmptyState } from '../design/primitives/EmptyState';
import { ProgressBar } from '../design/primitives/ProgressBar';
import { Skeleton } from '../design/primitives/Skeleton';
import { Page } from './Page';
import { Wordmark } from './Wordmark';
import s from './Status.module.css';

export function Splash() {
  return (
    <div className={s.splash} role="status" aria-label={t('boot.opening')}>
      <div className={s.splashInner}>
        <Wordmark size="lg" />
        <ProgressBar thin className={s.line} label={t('boot.opening')} />
      </div>
    </div>
  );
}

export function ViewFallback() {
  return (
    <div className={s.fallback} aria-busy="true" aria-label={t('common.loading')}>
      <Skeleton width="40%" height="2rem" />
      <Skeleton lines={3} />
      <Skeleton height="12rem" radius="lg" />
    </div>
  );
}

export function BootError({ message, otherTab = false }: { message: string; otherTab?: boolean }) {
  const mockHref = `${location.pathname}?mock=1${location.hash}`;
  return (
    <div className={s.error}>
      <div className={s.errorCard} role="alert">
        <Wordmark />
        <h1>{t(otherTab ? 'boot.otherTabTitle' : 'boot.failedTitle')}</h1>
        <p>{t(otherTab ? 'boot.otherTabBody' : 'boot.failedBody')}</p>
        {!otherTab && <code>{message}</code>}
        <div className={s.actions}>
          <Button variant="primary" icon="history" onClick={() => location.reload()}>{t('boot.retry')}</Button>
          <LinkButton variant="secondary" href={mockHref}>{t('boot.demo')}</LinkButton>
        </div>
      </div>
    </div>
  );
}

export function NotFound() {
  return (
    <Page title={t('notFound.title')} hideTitle>
      <EmptyState icon="palace" title={t('notFound.title')} body={t('notFound.body')} actions={<LinkButton href="#/" variant="primary" icon="home">{t('nav.home')}</LinkButton>} />
    </Page>
  );
}

export function IntegrityBanner({ problems, onDismiss }: { problems: string[]; onDismiss: () => void }) {
  return (
    <div role="alert" className={s.banner}>
      <strong>{t('integrity.problem')}</strong>
      <span className={s.bannerText}>{problems.join(' ')}</span>
      <LinkButton href="#/backups" size="sm" variant="secondary">{t('integrity.restore')}</LinkButton>
      <Button size="sm" variant="ghost" onClick={onDismiss}>{t('common.dismiss')}</Button>
    </div>
  );
}
