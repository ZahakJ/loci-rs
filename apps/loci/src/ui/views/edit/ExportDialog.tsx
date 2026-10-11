/**
 * Export one deck as .apkg with the two choices that matter (V-F15): include scheduling (due
 * dates and review history) and include media. Reusable: Home's deck menu opens it.
 */
import { useEffect, useState } from 'react';
import { t } from '@core/i18n/i18n';
import { api, downloadBytes, errorMessage } from '@ui/api';
import { Dialog } from '@ui/design/primitives/Dialog';
import { Field } from '@ui/design/primitives/Field';
import { ProgressBar } from '@ui/design/primitives/ProgressBar';
import { Switch } from '@ui/design/primitives/Switch';
import { toast } from '@ui/design/primitives/Toast';
import { formatPercent } from '@ui/hooks/format';
import { exportFileName } from './exportName';
import s from './ExportDialog.module.css';

export interface ExportDialogProps {
  open: boolean;
  onClose: () => void;
  deckId: number;
  /** full deck name (`Parent::Child`); the file is named after the last part */
  deckName: string;
}

export function ExportDialog({ open, onClose, deckId, deckName }: ExportDialogProps) {
  const [scheduling, setScheduling] = useState(true);
  const [media, setMedia] = useState(true);
  const [progress, setProgress] = useState<number | null>(null);
  const leaf = deckName.split('::').pop() ?? deckName;

  useEffect(() => { if (!open) setProgress(null); }, [open]);

  const run = async () => {
    setProgress(0);
    try {
      const bytes = await api.exportPackage({ deckId, includeScheduling: scheduling, includeMedia: media }, (p) => setProgress(p.total ? p.done / p.total : 0));
      downloadBytes(bytes, exportFileName(deckName));
      toast({ message: t('deck.exported', { name: leaf }), icon: 'download', tone: 'success' });
      onClose();
    } catch (e) {
      toast({ message: t('deck.exportFailed', { error: errorMessage(e) }), tone: 'danger' });
    } finally {
      setProgress(null);
    }
  };

  const busy = progress !== null;
  return (
    <Dialog
      open={open}
      onClose={() => { if (!busy) onClose(); }}
      title={t('exportDeck.title', { name: leaf })}
      description={t('exportDeck.help')}
      icon="export"
      confirmLabel={t('export.start')}
      busy={busy}
      onConfirm={run}
    >
      <div className={s.form}>
        <Field label={t('export.includeScheduling')} help={t('exportDeck.schedulingHelp')} inline>
          <Switch checked={scheduling} onChange={setScheduling} disabled={busy} />
        </Field>
        <Field label={t('export.includeMedia')} help={t('exportDeck.mediaHelp')} inline>
          <Switch checked={media} onChange={setMedia} disabled={busy} />
        </Field>
        {busy && <ProgressBar value={progress} label={t('export.progress', { percent: formatPercent(progress) })} thin />}
      </div>
    </Dialog>
  );
}
