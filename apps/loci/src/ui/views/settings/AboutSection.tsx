import { t } from '@core/i18n/i18n';
import { Icon } from '../../design/icons';
import { Card } from '../../design/primitives/Card';
import { APP_VERSION, LICENSE, REPO_URL } from '../../shell/version';
import { Section } from './SettingsParts';
import s from './Settings.module.css';

export function AboutSection() {
  return (
    <Section id="about" title={t('settings.about')} lede={t('about.lede')}>
      <Card padding="none">
        <dl className={s.about}>
          <dt>{t('about.version')}</dt><dd className="num">{APP_VERSION}</dd>
          <dt>{t('about.licence')}</dt><dd>{LICENSE}</dd>
          <dt>{t('about.source')}</dt>
          <dd><a href={REPO_URL} target="_blank" rel="noreferrer noopener">{REPO_URL.replace('https://', '')}<Icon name="external" size={14} style={{ display: 'inline', marginInlineStart: 4, verticalAlign: '-2px' }} /></a></dd>
          <dt>{t('about.storage')}</dt><dd>{t('about.storageValue')}</dd>
        </dl>
      </Card>
    </Section>
  );
}
