import { t } from '@core/i18n/i18n';
import { LinkButton } from '../design/primitives/Button';
import { EmptyState } from '../design/primitives/EmptyState';
import type { IconName } from '../design/icons';
import { Page } from './Page';

/** Temporary stand-in used by views another agent is still building. */
export function Placeholder({ title, icon = 'palace', detail }: { title: string; icon?: IconName; detail?: string }) {
  return (
    <Page title={title} hideTitle>
      <EmptyState
        icon={icon}
        title={t('placeholder.title', { view: title })}
        body={detail ?? t('placeholder.body')}
        actions={<LinkButton href="#/" variant="secondary" icon="home">{t('nav.home')}</LinkButton>}
      />
    </Page>
  );
}
