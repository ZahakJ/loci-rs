import { useRef, useState } from 'react';
import { t } from '@core/i18n/i18n';
import { errorMessage, useQuery } from '../../api';
import { Button } from '../../design/primitives/Button';
import { Card } from '../../design/primitives/Card';
import { toast } from '../../design/primitives/Toast';
import { formatRelative } from '../../hooks/format';
import { chooseFolder, describeSync, folderKind, forgetFolder, openSyncFile, saveSyncFile, syncNow, useSyncState } from '../../sync/sync';
import { Section, SettingRow } from './SettingsParts';
import s from './Settings.module.css';

export function SyncSection() {
  const status = useQuery((a) => a.syncStatus(), [], ['settings', 'decks', 'notes', 'cards']);
  const sync = useSyncState();
  const kind = folderKind();
  const [busy, setBusy] = useState<'choose' | 'save' | 'open' | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const folder = status.data?.folder ?? null;
  const last = status.data?.lastSync ?? null;

  const run = async (what: 'choose' | 'save' | 'open', fn: () => Promise<void>) => {
    setBusy(what);
    try { await fn(); } catch (e) { toast({ message: t('sync.failed', { error: errorMessage(e) }), tone: 'danger' }); }
    finally { setBusy(null); void status.refetch(); }
  };

  const now = () => run('choose', async () => {
    const r = await syncNow('manual');
    if (r) toast({ message: t('sync.done', { summary: describeSync(r) }), icon: 'check', tone: 'success' });
  });
  const choose = () => run('choose', async () => {
    const r = await chooseFolder();
    if (r) toast({ message: t('sync.done', { summary: describeSync(r) }), icon: 'check', tone: 'success' });
  });
  const stop = () => run('choose', async () => {
    await forgetFolder();
    toast({ message: t('sync.stopped'), icon: 'check' });
  });
  const save = () => run('save', saveSyncFile);
  const onFile = (file: File | undefined) => {
    if (!file) return;
    void run('open', async () => {
      const r = await openSyncFile(file);
      toast({ message: t('sync.done', { summary: describeSync(r) }), icon: 'check', tone: 'success' });
    });
  };

  const lastLine = sync.syncing ? t('sync.syncing') : last ? t('sync.last', { when: formatRelative(last) }) : t('sync.never');

  return (
    <Section id="sync" title={t('settings.sync')} lede={kind ? t('sync.lede') : t('sync.ledeFile')}>
      <Card padding="none" className={s.rows}>
        {kind && (
          <SettingRow label={t('sync.folder')} help={folder ? <>{folder}<br />{lastLine}</> : t('sync.folderHelp')}>
            {folder ? (
              <>
                <Button variant="primary" icon="cloud" loading={busy === 'choose' || sync.syncing} onClick={now}>{t('sync.now')}</Button>
                <Button variant="ghost" onClick={choose} disabled={!!busy}>{t('sync.change')}</Button>
                <Button variant="ghost" onClick={stop} disabled={!!busy}>{t('sync.stop')}</Button>
              </>
            ) : (
              <Button icon="cloud" loading={busy === 'choose'} onClick={choose}>{t('sync.choose')}</Button>
            )}
          </SettingRow>
        )}
        <SettingRow label={t('sync.save')} help={t('sync.saveHelp')}>
          <Button icon="upload" loading={busy === 'save'} onClick={save}>{t('sync.saveAction')}</Button>
        </SettingRow>
        <SettingRow label={t('sync.open')} help={t('sync.openHelp')}>
          <Button icon="download" loading={busy === 'open'} onClick={() => fileInput.current?.click()}>{t('sync.openAction')}</Button>
          <input
            ref={fileInput}
            type="file"
            hidden
            onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ''; onFile(f); }}
          />
        </SettingRow>
      </Card>
    </Section>
  );
}
