import { useEffect, useRef, useState } from 'react';
import { t } from '@core/i18n/i18n';
import { api, errorMessage } from '../../api';
import { Icon } from '../../design/icons';
import { Button } from '../../design/primitives/Button';
import { Card } from '../../design/primitives/Card';
import { Dialog } from '../../design/primitives/Dialog';
import { Field } from '../../design/primitives/Field';
import { Input } from '../../design/primitives/Input';
import { Pill } from '../../design/primitives/Pill';
import { useMedia } from '../../design/primitives/layer';
import { toast } from '../../design/primitives/Toast';
import { navigate } from '../../routes';
import { Wordmark } from '../../shell/Wordmark';
import { ThemeToggle } from '../../shell/ThemeToggle';
import { MANUAL_URL, openExternal } from '../../shell/native';
import { useImportFile } from '../../store/importFile';
import { DropZone } from '../import/DropZone';
import s from './Welcome.module.css';

function Motif({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 400 400" fill="none" stroke="currentColor" strokeWidth="1" aria-hidden>
      <path d="M40 150 C110 70 160 50 200 50 S290 70 360 150" />
      <path d="M30 160 H370 M30 172 H370" />
      {[80, 160, 240, 320].map((x) => <path key={x} d={`M${x - 14} 172 V352 M${x + 14} 172 V352`} />)}
      <path d="M20 352 H380 M10 366 H390" />
    </svg>
  );
}

/** First run (DESIGN §18): wordmark, one sentence, three calm options. */
export default function WelcomeView() {
  const setImportFile = useImportFile((st) => st.set);
  const [sampleBusy, setSampleBusy] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  const [name, setName] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const heading = useRef<HTMLHeadingElement>(null);
  // Narrow screens lead with the sample (the quickest way in); the DOM follows the visual order
  // so keyboard and screen-reader order match what is seen.
  const narrow = useMedia('(max-width: 899px)');

  useEffect(() => { document.title = t('welcome.docTitle'); }, []);

  const trySample = async () => {
    setSampleBusy(true);
    try {
      await api.installSampleDeck();
      toast({ message: t('welcome.sampleReady'), tone: 'success', icon: 'check' });
      navigate('#/', { replace: true });
    } catch (e) {
      toast({ message: errorMessage(e), tone: 'danger' });
      setSampleBusy(false);
    }
  };

  const create = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.createDeck(name.trim());
      setCreateOpen(false);
      navigate('#/', { replace: true });
    } catch (e) {
      setError(errorMessage(e));
      setBusy(false);
    }
  };

  const importCard = (
    <div className={s.optionPrimary} style={{ ['--i' as string]: narrow ? 2 : 0 }} data-option="import">
      <DropZone
        title={t('welcome.import.title')}
        hint={t('welcome.import.hint')}
        onFile={(file) => { setImportFile(file); navigate('#/import'); }}
      />
    </div>
  );
  const sampleCard = (
    <Card className={s.option} style={{ ['--i' as string]: narrow ? 0 : 1 }} data-option="sample">
      <span className={s.optionIcon} aria-hidden><Icon name="sparkle" /></span>
      <h2 className={s.optionTitle}>{t('welcome.sample.title')}</h2>
      <p className={s.optionBody}>{t('welcome.sample.body')}</p>
      <div className={s.tags} aria-label={t('welcome.sample.includes')}>
        <Pill>{t('welcome.sample.cloze')}</Pill><Pill>{t('welcome.sample.image')}</Pill><Pill>{t('welcome.sample.maths')}</Pill><Pill>{t('welcome.sample.arabic')}</Pill>
      </div>
      <div className={s.optionAction}>
        <Button variant="primary" loading={sampleBusy} onClick={trySample} autoFocus>{t('welcome.sample.action')}</Button>
      </div>
    </Card>
  );
  const emptyCard = (
    <Card className={s.option} style={{ ['--i' as string]: narrow ? 1 : 2 }} data-option="empty">
      <span className={s.optionIcon} aria-hidden><Icon name="plus" /></span>
      <h2 className={s.optionTitle}>{t('welcome.empty.title')}</h2>
      <p className={s.optionBody}>{t('welcome.empty.body')}</p>
      <div className={s.optionAction}>
        <Button variant="secondary" onClick={() => { setName(''); setCreateOpen(true); }}>{t('welcome.empty.action')}</Button>
      </div>
    </Card>
  );

  return (
    <div className={s.page}>
      <Motif className={s.motif} />
      <div className={s.inner}>
        <header className={`${s.intro} rise-in`}>
          <Wordmark size="lg" />
          <h1 ref={heading} className={s.lede} data-page-title tabIndex={-1}>
            {t('welcome.lede.before')}<em>{t('welcome.lede.em')}</em>{t('welcome.lede.after')}
          </h1>
          <p className={s.srs}>{t('welcome.srs')}</p>
          <p className={s.sub}>{t('welcome.sub')}</p>
        </header>

        <div className={`${s.options} stagger`}>
          {narrow ? <>{sampleCard}{emptyCard}{importCard}</> : <>{importCard}{sampleCard}{emptyCard}</>}
        </div>

        <footer className={s.foot}>
          <span className={s.private}><Icon name="shield" size={16} /><span>{t('welcome.private')}</span></span>
          <a className={s.manual} href={MANUAL_URL} onClick={(e) => { e.preventDefault(); void openExternal(MANUAL_URL); }}>
            <Icon name="help" size={16} /><span>{t('welcome.manual')}</span><Icon name="external" size={14} />
          </a>
          <span className={s.themeRow}><ThemeToggle size="sm" /></span>
        </footer>
      </div>

      <Dialog
        open={createOpen}
        onClose={() => setCreateOpen(false)}
        title={t('welcome.empty.dialogTitle')}
        icon="plus"
        confirmLabel={t('deck.create')}
        busy={busy}
        confirmDisabled={!name.trim()}
        onConfirm={create}
      >
        <Field label={t('deck.name')} help={t('deck.nameHelp')} error={error}>
          <Input value={name} onChange={(e) => setName(e.target.value)} placeholder={t('deck.namePlaceholder')} data-autofocus autoComplete="off" />
        </Field>
      </Dialog>
    </div>
  );
}
