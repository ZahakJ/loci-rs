import { useState } from 'react';
import { t } from '@core/i18n/i18n';
import type { BackupInfo } from '@core/types';
import { api, downloadBytes, errorMessage, useQuery } from '../../api';
import { Icon } from '../../design/icons';
import { Button } from '../../design/primitives/Button';
import { Card } from '../../design/primitives/Card';
import { Dialog } from '../../design/primitives/Dialog';
import { EmptyState } from '../../design/primitives/EmptyState';
import { Pill } from '../../design/primitives/Pill';
import { Skeleton } from '../../design/primitives/Skeleton';
import { toast } from '../../design/primitives/Toast';
import { formatBytes, formatCount, formatDateTime, formatRelative, localDateStamp } from '../../hooks/format';
import { navigate } from '../../routes';
import { Page } from '../../shell/Page';
import { useImportFile } from '../../store/importFile';
import s from './Backups.module.css';

const TONE: Record<BackupInfo['kind'], 'neutral' | 'accent'> = { daily: 'neutral', weekly: 'neutral', monthly: 'neutral', manual: 'accent', 'pre-restore': 'neutral' };

/** 'loci-backup-2026-10-07.zip', dated in local time. */
export function fullBackupName(d: Date = new Date()): string { return `loci-backup-${localDateStamp(d)}.zip`; }

export async function downloadFullBackup(): Promise<void> {
  try {
    const bytes = await api.exportFullBackup();
    // A Loci-format zip (collection + media + manifest): Loci restores it through Import › Replace.
    downloadBytes(bytes, fullBackupName(), 'application/zip');
    toast({ message: t('backups.downloaded'), icon: 'download', tone: 'success' });
  } catch (e) {
    toast({ message: t('backups.downloadFailed', { error: errorMessage(e) }), tone: 'danger' });
  }
}

export default function BackupsView() {
  const list = useQuery((a) => a.listBackups(), [], ['backups']);
  const [busyNow, setBusyNow] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [restore, setRestore] = useState<BackupInfo | null>(null);
  const [restoring, setRestoring] = useState(false);
  const setImportFile = useImportFile((st) => st.set);

  const backupNow = async () => {
    setBusyNow(true);
    try {
      const info = await api.backupNow('manual');
      toast({ message: t('backups.made', { size: formatBytes(info.bytes) }), icon: 'check', tone: 'success' });
      await list.refetch();
    } catch (e) {
      toast({ message: errorMessage(e), tone: 'danger' });
    } finally {
      setBusyNow(false);
    }
  };

  const doRestore = async () => {
    if (!restore) return;
    setRestoring(true);
    try {
      const report = await api.restoreBackup(restore.name);
      setRestore(null);
      toast({ message: report.ok ? t('backups.restored') : t('backups.restoredWithIssues'), icon: 'history', tone: report.ok ? 'success' : 'danger', duration: 8000 });
    } catch (e) {
      toast({ message: t('backups.restoreFailed', { error: errorMessage(e) }), tone: 'danger' });
    } finally {
      setRestoring(false);
    }
  };

  const items = list.data ?? [];
  return (
    <Page
      title={t('backups.title')}
      subtitle={t('backups.subtitle')}
      back={{ href: '#/settings?section=data', label: t('nav.settings') }}
      actions={(
        <>
          <Button variant="secondary" icon="download" loading={downloading} onClick={async () => { setDownloading(true); await downloadFullBackup(); setDownloading(false); }}>{t('backups.download')}</Button>
          <Button variant="primary" icon="archive" loading={busyNow} onClick={backupNow}>{t('backups.now')}</Button>
        </>
      )}
    >
      <div className={s.stack}>
        <Card padding="none" aria-busy={list.loading}>
          {list.loading ? (
            <div className={s.loading}><Skeleton lines={6} /></div>
          ) : items.length === 0 ? (
            <EmptyState compact icon="archive" title={t('backups.none')} body={t('backups.noneBody')} />
          ) : (
            <table className={s.table}>
              <caption className="sr-only">{t('backups.caption')}</caption>
              <thead>
                <tr>
                  <th scope="col">{t('backups.col.when')}</th>
                  <th scope="col">{t('backups.col.kind')}</th>
                  <th scope="col" className={s.num}>{t('backups.col.cards')}</th>
                  <th scope="col" className={s.num}>{t('backups.col.size')}</th>
                  <th scope="col"><span className="sr-only">{t('backups.col.actions')}</span></th>
                </tr>
              </thead>
              <tbody>
                {items.map((b, i) => (
                  <tr key={b.name}>
                    <td>
                      <div className={s.when}>
                        <span className={s.whenRel}>{capitalise(formatRelative(b.at))}{i === 0 && <Pill tone="good" className={s.latest}>{t('backups.latest')}</Pill>}</span>
                        <span className={s.whenAbs}>{formatDateTime(b.at)}</span>
                        <span className={s.metaPhone}>{t(`backups.kind.${b.kind}`)} · {b.cards === null ? '–' : t('backups.cards', { n: formatCount(b.cards) })} · {formatBytes(b.bytes)}</span>
                      </div>
                    </td>
                    <td><Pill tone={TONE[b.kind]} outline={b.kind === 'pre-restore'}>{t(`backups.kind.${b.kind}`)}</Pill></td>
                    <td className={s.num}>{b.cards === null ? '–' : t('backups.cards', { n: formatCount(b.cards) })}</td>
                    <td className={s.num}>{formatBytes(b.bytes)}</td>
                    <td className={s.act}>
                      <Button size="sm" variant="secondary" icon="history" onClick={() => setRestore(b)} aria-label={t('backups.restoreNamed', { when: formatDateTime(b.at) })}>
                        {t('backups.restore')}
                      </Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Card>
        <div className={s.foot}>
          <span className={s.policy}><Icon name="shield" size={18} />{t('backups.policy')}</span>
          <Button variant="ghost" icon="upload" onClick={() => { setImportFile(null, 'replace'); navigate('#/import'); }}>{t('backups.fromFile')}</Button>
        </div>
        <p className={s.lociOnly}><Icon name="info" size={16} /><span>{t('backups.lociOnly')}{t('backups.forAnki.before')}<a href="#/settings?section=data">{t('backups.forAnki.link')}</a>{t('backups.forAnki.after')}</span></p>
      </div>

      <Dialog
        open={!!restore}
        onClose={() => setRestore(null)}
        title={restore ? t('backups.confirmTitle', { when: formatDateTime(restore.at) }) : ''}
        icon="history"
        confirmLabel={t('backups.restore')}
        busy={restoring}
        onConfirm={doRestore}
        description={restore ? t('backups.confirmBody', { date: formatDateTime(restore.at), cards: restore.cards === null ? '?' : formatCount(restore.cards) }) : ''}
      >
        <ul role="list" className={s.restoreList}>
          <li><Icon name="check" size={16} />{t('backups.confirmSafe')}</li>
          <li><Icon name="check" size={16} />{t('backups.confirmMedia')}</li>
        </ul>
      </Dialog>
    </Page>
  );
}

function capitalise(s: string) { return s.charAt(0).toLocaleUpperCase() + s.slice(1); }
