import { useState } from 'react';
import { t } from '@core/i18n/i18n';
import type { IntegrityReport } from '@core/types';
import { api, downloadBytes, errorMessage } from '../../api';
import { Icon, type IconName } from '../../design/icons';
import { Button } from '../../design/primitives/Button';
import { Card } from '../../design/primitives/Card';
import { Dialog } from '../../design/primitives/Dialog';
import { Field } from '../../design/primitives/Field';
import { Input } from '../../design/primitives/Input';
import { ProgressBar } from '../../design/primitives/ProgressBar';
import { Switch } from '../../design/primitives/Switch';
import { toast } from '../../design/primitives/Toast';
import { formatCount, localDateStamp } from '../../hooks/format';
import { navigate } from '../../routes';
import { downloadFullBackup } from '../backups/BackupsView';
import { Section, SettingRow } from './SettingsParts';
import s from './Settings.module.css';

function LinkRow({ href, icon, label, help }: { href: string; icon: IconName; label: string; help: string }) {
  return (
    <a href={href} className={s.linkRow}>
      <Icon name={icon} />
      <span className={s.text}><span className={s.label}>{label}</span><span className={s.help}>{help}</span></span>
      <Icon name="chevron-right" size={18} />
    </a>
  );
}

export function DataSection() {
  const [exportOpen, setExportOpen] = useState(false);
  const [exportOpts, setExportOpts] = useState({ includeScheduling: true, includeMedia: true });
  const [exportProgress, setExportProgress] = useState<number | null>(null);
  const [fullBusy, setFullBusy] = useState(false);
  const [media, setMedia] = useState<{ unused: string[]; missing: string[] } | null>(null);
  const [mediaBusy, setMediaBusy] = useState(false);
  const [integrity, setIntegrity] = useState<IntegrityReport | null>(null);
  const [integrityBusy, setIntegrityBusy] = useState(false);
  const [reset, setReset] = useState<0 | 1 | 2>(0);
  const [resetText, setResetText] = useState('');
  const [resetBusy, setResetBusy] = useState(false);
  const resetWord = t('data.resetWord');

  const doExport = async () => {
    setExportProgress(0);
    try {
      const bytes = await api.exportPackage({ deckId: null, ...exportOpts }, (p) => setExportProgress(p.total ? p.done / p.total : 0));
      downloadBytes(bytes, `loci-collection-${localDateStamp()}.apkg`);
      toast({ message: t('data.exported'), icon: 'download', tone: 'success' });
      setExportOpen(false);
    } catch (e) {
      toast({ message: t('data.exportFailed', { error: errorMessage(e) }), tone: 'danger' });
    } finally {
      setExportProgress(null);
    }
  };

  const checkMedia = async () => {
    setMediaBusy(true);
    try { setMedia(await api.checkMedia()); }
    catch (e) { toast({ message: errorMessage(e), tone: 'danger' }); }
    finally { setMediaBusy(false); }
  };
  const checkIntegrity = async () => {
    setIntegrityBusy(true);
    try { setIntegrity(await api.checkIntegrity()); }
    catch (e) { toast({ message: errorMessage(e), tone: 'danger' }); }
    finally { setIntegrityBusy(false); }
  };
  const doReset = async () => {
    setResetBusy(true);
    try {
      await api.resetCollection();
      setReset(0);
      toast({ message: t('data.resetDone'), icon: 'check' });
      navigate('#/welcome', { replace: true });
    } catch (e) {
      toast({ message: errorMessage(e), tone: 'danger' });
      setResetBusy(false);
    }
  };

  return (
    <Section id="data" title={t('settings.data')} lede={t('data.lede')}>
      <Card padding="none" className={s.rows}>
        <LinkRow href="#/backups" icon="archive" label={t('data.backups')} help={t('data.backupsHelp')} />
        <LinkRow href="#/import" icon="import" label={t('data.import')} help={t('data.importHelp')} />
        <SettingRow label={t('data.export')} help={t('data.exportHelp')}>
          <Button icon="export" onClick={() => setExportOpen(true)}>{t('data.exportAction')}</Button>
        </SettingRow>
        <SettingRow label={t('data.full')} help={t('data.fullHelp')}>
          <Button icon="download" loading={fullBusy} onClick={async () => { setFullBusy(true); await downloadFullBackup(); setFullBusy(false); }}>{t('data.fullAction')}</Button>
        </SettingRow>
        <SettingRow label={t('data.media')} help={t('data.mediaHelp')}>
          <Button icon="image" loading={mediaBusy} onClick={checkMedia}>{t('data.check')}</Button>
        </SettingRow>
        <SettingRow label={t('data.integrity')} help={t('data.integrityHelp')}>
          <Button icon="shield" loading={integrityBusy} onClick={checkIntegrity}>{t('data.check')}</Button>
        </SettingRow>
        <SettingRow label={<span className={s.danger}>{t('data.reset')}</span>} help={t('data.resetHelp')}>
          <Button variant="quiet-danger" icon="trash" onClick={() => { setResetText(''); setReset(1); }}>{t('data.resetAction')}</Button>
        </SettingRow>
      </Card>

      <Dialog
        open={exportOpen}
        onClose={() => exportProgress === null && setExportOpen(false)}
        title={t('data.exportTitle')}
        icon="export"
        description={t('data.exportBody')}
        confirmLabel={t('data.exportConfirm')}
        busy={exportProgress !== null}
        onConfirm={doExport}
      >
        <div className={s.report}>
          <Field label={t('data.includeScheduling')} help={t('data.includeSchedulingHelp')} inline>
            <Switch checked={exportOpts.includeScheduling} onChange={(v) => setExportOpts({ ...exportOpts, includeScheduling: v })} />
          </Field>
          <Field label={t('data.includeMedia')} inline>
            <Switch checked={exportOpts.includeMedia} onChange={(v) => setExportOpts({ ...exportOpts, includeMedia: v })} />
          </Field>
          {exportProgress !== null && <ProgressBar value={exportProgress} label={t('data.exporting')} />}
        </div>
      </Dialog>

      <Dialog
        open={!!media}
        onClose={() => setMedia(null)}
        title={t('data.mediaTitle')}
        icon="image"
        actions={(
          <>
            {media && media.unused.length > 0 && (
              <Button variant="quiet-danger" onClick={async () => { await api.deleteMedia(media.unused); toast({ message: t('data.mediaDeleted', { n: media.unused.length }), icon: 'trash' }); setMedia(null); }}>
                {t('data.deleteUnused', { n: media.unused.length })}
              </Button>
            )}
            <Button variant="primary" onClick={() => setMedia(null)} data-autofocus>{t('common.done')}</Button>
          </>
        )}
      >
        {media && (
          <div className={s.report}>
            {media.unused.length === 0 && media.missing.length === 0 && <p className={s.okLine}><Icon name="check" />{t('data.mediaOk')}</p>}
            {media.missing.length > 0 && (<><p>{t('data.missing', { count: media.missing.length, n: formatCount(media.missing.length) })}</p><ul className={s.reportList}>{media.missing.map((m) => <li key={m}>{m}</li>)}</ul></>)}
            {media.unused.length > 0 && (<><p>{t('data.unused', { count: media.unused.length, n: formatCount(media.unused.length) })}</p><ul className={s.reportList}>{media.unused.map((m) => <li key={m}>{m}</li>)}</ul></>)}
          </div>
        )}
      </Dialog>

      <Dialog
        open={!!integrity}
        onClose={() => setIntegrity(null)}
        title={integrity?.ok ? t('data.integrityOk') : t('data.integrityBad')}
        icon={integrity?.ok ? 'shield' : 'warning'}
        actions={<Button variant="primary" onClick={() => setIntegrity(null)} data-autofocus>{t('common.done')}</Button>}
      >
        {integrity && (
          <div className={s.report}>
            {integrity.ok && integrity.repaired.length === 0 && <p className={s.okLine}><Icon name="check" />{t('data.integrityClean')}</p>}
            {integrity.problems.length > 0 && <ul className={s.reportList}>{integrity.problems.map((p) => <li key={p}>{p}</li>)}</ul>}
            {integrity.repaired.length > 0 && (<><p>{t('data.repaired')}</p><ul className={s.reportList}>{integrity.repaired.map((p) => <li key={p}>{p}</li>)}</ul></>)}
            {!integrity.ok && <p>{t('data.integrityAdvice')} <a href="#/backups">{t('data.backups')}</a></p>}
          </div>
        )}
      </Dialog>

      <Dialog
        open={reset === 1}
        onClose={() => setReset(0)}
        destructive
        title={t('data.reset1Title')}
        description={t('data.reset1Body')}
        confirmLabel={t('data.reset1Confirm')}
        onConfirm={() => setReset(2)}
      />
      <Dialog
        open={reset === 2}
        onClose={() => setReset(0)}
        destructive
        title={t('data.reset2Title')}
        description={t('data.reset2Body', { word: resetWord })}
        confirmLabel={t('data.resetAction')}
        busy={resetBusy}
        confirmDisabled={resetText.trim().toUpperCase() !== resetWord.toUpperCase()}
        onConfirm={doReset}
      >
        <Field label={t('data.reset2Label', { word: resetWord })}>
          <Input value={resetText} onChange={(e) => setResetText(e.target.value)} autoComplete="off" data-autofocus />
        </Field>
      </Dialog>
    </Section>
  );
}
