import { useEffect, useRef, useState, type CSSProperties } from 'react';
import { t } from '@core/i18n/i18n';
import type { ImportOptions, ImportPreview, ImportReport, Progress } from '@core/types';
import { api, errorCode, errorMessage } from '../../api';
import { Icon } from '../../design/icons';
import { Button, LinkButton } from '../../design/primitives/Button';
import { Card } from '../../design/primitives/Card';
import { Field } from '../../design/primitives/Field';
import { ProgressBar } from '../../design/primitives/ProgressBar';
import { Segmented } from '../../design/primitives/Segmented';
import { Select } from '../../design/primitives/Select';
import { Skeleton } from '../../design/primitives/Skeleton';
import { Switch } from '../../design/primitives/Switch';
import { cx } from '../../design/primitives/layer';
import { formatBytes, formatCount } from '../../hooks/format';
import { navigate } from '../../routes';
import { Page } from '../../shell/Page';
import { useImportFile } from '../../store/importFile';
import { importButtonLabel, isLociBackupFile, uniqueWarnings, type PreviewCounts } from './importText';
import { DropZone } from './DropZone';
import s from './Import.module.css';

type Step =
  | { kind: 'pick' }
  | { kind: 'reading'; file: File }
  | { kind: 'preview'; file: File; bytes: Uint8Array; preview: ImportPreview }
  | { kind: 'importing'; file: File; progress: Progress | null; phases: string[] }
  | { kind: 'done'; file: File; report: ImportReport; shown: string[] }
  | { kind: 'error'; file: File | null; code: string; message: string };

const PHASES = ['Reading package', 'Notes', 'Cards', 'Review history', 'Media', 'Checking integrity'];

export default function ImportView() {
  const take = useImportFile((st) => st.take);
  const [step, setStep] = useState<Step>({ kind: 'pick' });
  const [options, setOptions] = useState<ImportOptions>({ mode: 'merge', keepScheduling: true, conflict: 'newer' });
  const live = useRef<HTMLDivElement>(null);

  const read = async (file: File) => {
    setStep({ kind: 'reading', file });
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      const preview = await api.previewImport(bytes);
      const backup = isLociBackupFile(file.name);
      setOptions((o) => ({ ...o, keepScheduling: preview.hasScheduling, mode: backup ? 'replace' : preview.isCollectionPackage ? o.mode : 'merge' }));
      setStep({ kind: 'preview', file, bytes, preview });
    } catch (e) {
      setStep({ kind: 'error', file, code: errorCode(e) ?? 'internal', message: errorMessage(e) });
    }
  };

  // a file handed over from Welcome (or Backups → restore from file)
  useEffect(() => {
    const { file, mode } = take();
    if (mode) setOptions((o) => ({ ...o, mode }));
    if (file) void read(file);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const runImport = async () => {
    if (step.kind !== 'preview') return;
    const { file, bytes } = step;
    const seen: string[] = [];
    setStep({ kind: 'importing', file, progress: null, phases: seen });
    try {
      const report = await api.importPackage(bytes, options, (p) => {
        if (!seen.includes(p.phase)) seen.push(p.phase);
        setStep({ kind: 'importing', file, progress: p, phases: [...seen] });
      });
      setStep({ kind: 'done', file, report, shown: step.preview.warnings });
    } catch (e) {
      setStep({ kind: 'error', file, code: errorCode(e) ?? 'internal', message: errorMessage(e) });
    }
  };

  const reset = () => setStep({ kind: 'pick' });

  return (
    <Page
      title={t('import.title')}
      subtitle={t('import.subtitle')}
      back={{ href: '#/settings?section=data', label: t('nav.settings') }}
      width="narrow"
    >
      <div className={s.stack} ref={live} aria-live="polite" aria-busy={step.kind === 'reading' || step.kind === 'importing'}>
        {step.kind === 'pick' && (
          <>
            <DropZone onFile={read} autoFocus />
            <ul role="list" className={s.explain}>
              <li><Icon name="calendar" /><div><strong>{t('import.explain.dueTitle')}</strong>{t('import.explain.due')}</div></li>
              <li><Icon name="shield" /><div><strong>{t('import.explain.safeTitle')}</strong>{t('import.explain.safe')}</div></li>
              <li><Icon name="export" /><div><strong>{t('import.explain.exportTitle')}</strong>{t('import.explain.export')}</div></li>
            </ul>
          </>
        )}

        {step.kind === 'reading' && (
          <Card className={s.preview}>
            <FileRow file={step.file} meta={t('import.reading')} />
            <Skeleton height="5.5rem" radius="md" />
            <Skeleton lines={3} />
          </Card>
        )}

        {step.kind === 'preview' && (
          <Card className={s.preview}>
            <FileRow file={step.file} meta={isLociBackupFile(step.file.name) ? t('import.layout.lociBackup') : layoutLabel(step.preview)} />
            <PreviewNumbers preview={step.preview} replace={options.mode === 'replace'} />
            <div className={s.cols}>
              <div>
                <h2 className={s.listTitle}>{t('import.decks', { count: step.preview.decks.length })}</h2>
                <ul role="list" className={s.deckList}>
                  {step.preview.decks.slice(0, 10).map((d) => (
                    <li key={d} style={{ '--level': d.split('::').length - 1 } as CSSProperties}><Icon name="cards" size={14} />{d.split('::').pop()}</li>
                  ))}
                </ul>
                {step.preview.decks.length > 10 && <p className={s.more}>{t('import.andMore', { n: step.preview.decks.length - 10 })}</p>}
              </div>
              <div>
                <h2 className={s.listTitle}>{t('import.notetypes')}</h2>
                <ul role="list" className={s.deckList}>
                  {step.preview.notetypes.map((n) => <li key={n}><Icon name="layers" size={14} />{n}</li>)}
                </ul>
              </div>
            </div>
            {step.preview.hasScheduling ? (
              <div className={s.sched}>
                <Icon name="check" />
                <div>
                  {t('import.schedFound')}
                  {step.preview.fsrsCards > 0 && <small>{t('import.fsrsCards', { n: formatCount(step.preview.fsrsCards) })}</small>}
                </div>
              </div>
            ) : (
              <div className={`${s.sched} ${s.schedNone}`}><Icon name="info" /><div>{t('import.schedNone')}</div></div>
            )}
            {uniqueWarnings(step.preview.warnings).length > 0 && (
              <ul role="list" className={s.warnings}>
                {uniqueWarnings(step.preview.warnings).map((w) => <li key={w} className={s.warning}><Icon name="warning" size={16} />{w}</li>)}
              </ul>
            )}

            <div className={s.options}>
              <h2 className={s.optionsTitle}>{t('import.options')}</h2>
              <Field label={t('import.mode')} help={options.mode === 'merge' ? t('import.mode.mergeHelp') : undefined}>
                <Segmented
                  block
                  label={t('import.mode')}
                  value={options.mode}
                  onChange={(mode) => setOptions({ ...options, mode })}
                  options={[
                    { value: 'merge', label: t('import.mode.merge'), disabled: isLociBackupFile(step.file.name) },
                    { value: 'replace', label: t('import.mode.replace') },
                  ]}
                />
              </Field>
              {options.mode === 'replace' && (
                <p className={s.replaceNote}><Icon name="shield" size={18} />{t('import.mode.replaceHelp')}</p>
              )}
              <Field label={t('import.keepScheduling')} help={step.preview.hasScheduling ? t('import.keepSchedulingHelp') : t('import.keepSchedulingNone')} inline>
                <Switch checked={options.keepScheduling} disabled={!step.preview.hasScheduling} onChange={(keepScheduling) => setOptions({ ...options, keepScheduling })} />
              </Field>
              {options.mode === 'merge' && (
                <Field label={t('import.conflict')} help={t('import.conflictHelp')}>
                  <Select
                    value={options.conflict}
                    onChange={(conflict) => setOptions({ ...options, conflict })}
                    options={[
                      { value: 'newer', label: t('import.conflict.newer') },
                      { value: 'keep', label: t('import.conflict.keep') },
                      { value: 'theirs', label: t('import.conflict.theirs') },
                    ]}
                  />
                </Field>
              )}
              <div className={s.actions}>
                {(() => {
                  const label = importButtonLabel(step.preview, options.mode);
                  return (
                    <Button variant="primary" size="lg" icon="import" onClick={runImport} autoFocus={label !== null} disabled={label === null}>
                      {label ?? t('import.nothingNew')}
                    </Button>
                  );
                })()}
                <Button variant="ghost" size="lg" onClick={reset}>{t('import.another')}</Button>
              </div>
            </div>
          </Card>
        )}

        {step.kind === 'importing' && (
          <Card className={s.progressCard}>
            <FileRow file={step.file} meta={t('import.importing')} />
            <ProgressBar
              value={step.progress && step.progress.total > 0 ? overall(step.progress, step.phases) : undefined}
              label={step.progress ? phaseLabel(step.progress.phase) : t('import.starting')}
              detail={step.progress && step.progress.total > 4 ? `${formatCount(step.progress.done)} / ${formatCount(step.progress.total)}` : undefined}
            />
            <ol className={s.phases} aria-label={t('import.steps')}>
              {PHASES.map((p) => {
                const state = step.progress?.phase === p ? 'active' : step.phases.includes(p) || step.progress?.phase === 'Done' ? 'done' : 'todo';
                return (
                  <li key={p} className={s.phase} data-state={state}>
                    {state === 'done' ? <Icon name="check" size={16} /> : <span className={s.phaseDot} aria-hidden />}
                    {phaseLabel(p)}
                  </li>
                );
              })}
            </ol>
            <p className={s.safe}><Icon name="shield" size={16} />{t('import.safeWhile')}</p>
          </Card>
        )}

        {step.kind === 'done' && (
          <Card className={s.done}>
            <div className={s.doneHead}>
              <span className={s.doneMark} aria-hidden><Icon name="check" size={24} /></span>
              <div>
                <h2 className={s.doneTitle}>{t('import.doneTitle', { n: formatCount(step.report.notesAdded + step.report.notesUpdated) })}</h2>
                <p className={s.doneSub}>{t('import.doneSub', { seconds: (step.report.durationMs / 1000).toFixed(1) })}</p>
              </div>
            </div>
            <div className={s.numbers}>
              <Num value={step.report.notesAdded} label={t('import.r.notesAdded')} />
              <Num value={step.report.notesUpdated} label={t('import.r.notesUpdated')} />
              <Num value={step.report.cardsAdded} label={t('import.r.cards')} />
              <Num value={step.report.reviewsAdded} label={t('import.r.reviews')} />
              <Num value={step.report.mediaAdded} label={t('import.r.media')} />
            </div>
            {uniqueWarnings(step.report.warnings, step.shown).length > 0 && (
              <ul role="list" className={s.warnings}>
                {uniqueWarnings(step.report.warnings, step.shown).map((w) => <li key={w} className={s.warning}><Icon name="warning" size={16} />{w}</li>)}
              </ul>
            )}
            <div className={s.actions}>
              <Button variant="primary" size="lg" icon="play" kbd="enter" autoFocus onClick={() => navigate('#/study/all')}>{t('import.startStudying')}</Button>
              <LinkButton variant="ghost" size="lg" href="#/">{t('import.goHome')}</LinkButton>
            </div>
          </Card>
        )}

        {step.kind === 'error' && (
          <Card className={s.errorCard} role="alert">
            <span className={s.errorMark} aria-hidden><Icon name="warning" /></span>
            <h2 className={s.errorTitle}>{t(`import.err.${errKey(step.code)}.title`)}</h2>
            <p className={s.errorBody}>{t(`import.err.${errKey(step.code)}.body`)}</p>
            <p className={s.safe}><Icon name="shield" size={16} />{t('import.unchanged')}</p>
            {errKey(step.code) === 'other' && <p className={s.errorDetail}>{step.message}</p>}
            <div className={s.actions}>
              <Button variant="primary" onClick={reset} autoFocus>{t('import.another')}</Button>
              {/* a file in the wrong format stays wrong; only an unexpected failure is worth retrying */}
              {step.file && errKey(step.code) === 'other' && <Button variant="ghost" onClick={() => void read(step.file!)}>{t('common.tryAgain')}</Button>}
            </div>
          </Card>
        )}
      </div>
    </Page>
  );
}

function errKey(code: string): 'unsupported' | 'corrupt' | 'other' {
  return code === 'unsupported-package' ? 'unsupported' : code === 'corrupt' ? 'corrupt' : 'other';
}

function phaseLabel(phase: string): string {
  const key = `import.phase.${phase.toLowerCase().replace(/[^a-z]+/g, '-')}`;
  const v = t(key);
  return v === key ? phase : v;
}

/** Overall progress across known phases (each phase is an equal share). */
function overall(p: Progress, seen: string[]): number {
  if (p.phase === 'Done') return 1;
  const idx = Math.max(0, PHASES.indexOf(p.phase));
  const within = p.total > 0 ? p.done / p.total : 0;
  void seen;
  return Math.min(1, (idx + within) / PHASES.length);
}

function layoutLabel(p: ImportPreview): string {
  const layout = t(`import.layout.${p.layout}`);
  return p.isCollectionPackage ? `${t('import.layout.colpkg')} · ${layout}` : `${t('import.layout.apkg')} · ${layout}`;
}

function FileRow({ file, meta }: { file: File; meta: string }) {
  return (
    <div className={s.fileRow}>
      <span className={s.fileIcon} aria-hidden><Icon name="file" /></span>
      <div className={s.fileText}>
        <div className={s.fileName}>{file.name}</div>
        <div className={s.fileMeta}>{formatBytes(file.size)} · {meta}</div>
      </div>
    </div>
  );
}

function Num({ value, label }: { value: number; label: string }) {
  return (
    <div className={s.number}>
      <span className={cx(s.numberValue, 'num-lg')}>{formatCount(value)}</span>
      <span className={s.numberLabel}>{label}</span>
    </div>
  );
}

function PreviewNumbers({ preview, replace }: { preview: PreviewCounts; replace: boolean }) {
  const known = !replace && typeof preview.notesExisting === 'number';
  return (
    <div className={s.numbers}>
      <Num value={preview.notes} label={t('import.n.notes')} />
      {known && <Num value={preview.notesExisting!} label={t('import.n.existing')} />}
      <Num value={preview.cards} label={t('import.n.cards')} />
      <Num value={preview.decks.length} label={t('import.n.decks')} />
      <Num value={preview.reviews} label={t('import.n.reviews')} />
      <Num value={preview.media} label={t('import.n.media')} />
    </div>
  );
}
