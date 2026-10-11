import { useEffect, useMemo, useRef, useState, type MouseEvent, type ReactNode } from 'react';
import { t } from '@core/i18n/i18n';
import type { DeckConfig, DeckConfigPreset, OptimiseResult } from '@core/types';
import { api, errorMessage, useQuery } from '../../api';
import { Button } from '../../design/primitives/Button';
import { Card } from '../../design/primitives/Card';
import { Dialog } from '../../design/primitives/Dialog';
import { EmptyState } from '../../design/primitives/EmptyState';
import { Field } from '../../design/primitives/Field';
import { Input, Textarea } from '../../design/primitives/Input';
import { cx } from '../../design/primitives/layer';
import { ProgressBar } from '../../design/primitives/ProgressBar';
import { Select } from '../../design/primitives/Select';
import { Skeleton } from '../../design/primitives/Skeleton';
import { Slider } from '../../design/primitives/Slider';
import { Switch } from '../../design/primitives/Switch';
import { toast } from '../../design/primitives/Toast';
import { useBottomBar } from '../../design/primitives/bottomBar';
import { useUi } from '../../store/ui';
import { formatCount, formatPercent, formatSteps, parseSteps } from '../../hooks/format';
import { navigate } from '../../routes';
import { Page } from '../../shell/Page';
import { FSRS6_DEFAULT, GROUPS, PRESET_FIELDS, STANDARD, isRecommended, parseParams, type PresetField } from './presetFields';
import { Section, SettingRow, SideIndex } from './SettingsParts';
import { decksUsing } from '../../hooks/presets';
import { RetentionForecast } from './RetentionForecast';
import { lossChange, unsavedEdits } from './presetEdit';
import ps from './PresetEditor.module.css';
import s from './Settings.module.css';

function displayValue(f: PresetField, v: unknown): string {
  const k = f.kind;
  switch (k.type) {
    case 'bool': return v ? t('common.on') : t('common.off');
    case 'percent': return formatPercent(v as number);
    case 'permille': return formatPercent((v as number) / 1000);
    case 'steps': return formatSteps(v as number[]);
    case 'select': return t(`preset.opt.${String(f.key)}.${String(v)}`);
    case 'float': return `${v}${k.unit ?? ''}`;
    case 'int': return `${formatCount(v as number)}${k.unit ? ` ${t(`unit.${k.unit}`)}` : ''}`;
    default: return '';
  }
}

export default function PresetEditorView({ presetId }: { presetId: number }) {
  const q = useQuery((a) => a.getPreset(presetId), [presetId], 'none');
  const tree = useQuery((a) => a.deckTree(), [], ['decks', 'presets']);
  const [name, setName] = useState('');
  const [draft, setDraft] = useState<DeckConfig | null>(null);
  const [saving, setSaving] = useState(false);
  const [confirmLeave, setConfirmLeave] = useState<string | null>(null);

  // A reload after Optimise hands over the merged draft (stored values + unsaved edits) here.
  const pendingDraft = useRef<DeckConfig | null>(null);
  useEffect(() => {
    if (!q.data) return;
    const pending = pendingDraft.current;
    pendingDraft.current = null;
    setDraft(pending ?? q.data.config);
    if (!pending) setName(q.data.name);
  }, [q.data]);

  const dirty = !!q.data && !!draft && (name !== q.data.name || JSON.stringify(draft) !== JSON.stringify(q.data.config));

  useEffect(() => {
    if (!dirty) return;
    const onBefore = (e: BeforeUnloadEvent) => { e.preventDefault(); };
    window.addEventListener('beforeunload', onBefore);
    return () => window.removeEventListener('beforeunload', onBefore);
  }, [dirty]);

  const save = async () => {
    if (!draft || !q.data) return;
    setSaving(true);
    try {
      const saved = await api.savePreset({ id: q.data.id, name: name.trim() || q.data.name, config: draft });
      q.setData(saved);
      toast({ message: t('preset.saved', { name: saved.name }), icon: 'check', tone: 'success' });
    } catch (e) {
      toast({ message: t('common.saveFailedDetail', { error: errorMessage(e) }), tone: 'danger' });
    } finally {
      setSaving(false);
    }
  };

  // Ctrl/Cmd+S saves
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') { e.preventDefault(); if (dirty) void save(); }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  const groups = useMemo(() => GROUPS.map((g) => ({ id: g, label: t(`preset.group.${g}`) })), []);
  const saveBarRef = useRef<HTMLDivElement>(null);
  useBottomBar(saveBarRef, dirty);

  /**
   * Re-reads the stored preset (after Optimise or its undo) into the form, keeping unsaved edits to
   * other fields, so the form shows what is stored and Discard reverts only what the user typed.
   */
  const reloadPreset = async (opts: { keepEdits: boolean }) => {
    const fresh = await api.getPreset(presetId);
    const edits = opts.keepEdits && q.data && draft ? unsavedEdits(q.data.config, draft) : {};
    delete edits.fsrsParams;
    pendingDraft.current = { ...fresh.config, ...edits };
    q.setData(fresh);
  };

  const onOptimised = async (r: OptimiseResult) => {
    const presetName = q.data?.name ?? '';
    if (!r.adopted) {
      toast({ message: t('preset.optimiseKept', { name: presetName }), icon: 'info' });
      return;
    }
    await reloadPreset({ keepEdits: true }).catch(() => undefined);
    toast({
      message: t('preset.optimisedFor', { name: presetName, change: lossChange(r.before.logLoss, r.after.logLoss) }),
      icon: 'sparkle',
      tone: 'success',
      duration: 12000,
      action: {
        label: t('common.undo'),
        onClick: () => {
          void api.undo()
            .then(() => reloadPreset({ keepEdits: true }))
            .then(() => toast({ message: t('preset.optimiseUndone', { name: presetName }), icon: 'undo' }))
            .catch((e: unknown) => toast({ message: t('undo.failed') + ' ' + errorMessage(e), tone: 'danger' }));
        },
      },
    });
  };
  const leave = (href: string) => (e: MouseEvent) => { if (dirty) { e.preventDefault(); setConfirmLeave(href); } };

  if (q.error) {
    return (
      <Page title={t('preset.notFound')} back={{ href: '#/settings?section=advanced', label: t('nav.settings') }}>
        <EmptyState icon="settings" title={t('preset.notFound')} body={errorMessage(q.error)} />
      </Page>
    );
  }

  const using = decksUsing(tree.data, presetId);
  const set = <K extends keyof DeckConfig>(key: K, value: DeckConfig[K]) => setDraft((d) => (d ? { ...d, [key]: value } : d));

  return (
    <Page
      title={q.data ? q.data.name : t('preset.title')}
      eyebrow={t('preset.eyebrow')}
      subtitle={t('preset.subtitle')}
      back={{ href: '#/settings?section=advanced', label: t('nav.settings') }}
      width="wide"
    >
      <div className={s.layout} onClickCapture={(e) => {
        const a = (e.target as HTMLElement).closest('a[href^="#/"]');
        if (a && dirty) leave(a.getAttribute('href')!)(e as unknown as MouseEvent);
      }}>
        <SideIndex items={groups} label={t('preset.groups')} hrefFor={() => `#/settings/presets/${presetId}`} ready={!!draft && !!q.data} />
        <div className={s.sections}>
          {!draft || !q.data ? <Skeleton height="30rem" radius="lg" /> : (
            <>
              <div className={ps.head}>
                <Field label={t('presets.name')} className={ps.nameField}>
                  <Input value={name} onChange={(e) => setName(e.target.value)} autoComplete="off" />
                </Field>
                <p className={ps.usage}>
                  {using.length ? t('preset.usedBy', { list: using.map((n) => n.split('::').pop()).join(', ') }) : t('settings.usedByNone')}
                </p>
              </div>
              {GROUPS.map((g) => (
                <Section key={g} id={g} title={t(`preset.group.${g}`)} lede={t(`preset.group.${g}.lede`)}>
                  <Card padding="none" className={s.rows}>
                    {PRESET_FIELDS.filter((f) => f.group === g).map((f) => (
                      <FieldRow key={f.key} field={f} config={draft} set={set} presetId={q.data!.id} onParams={(params) => set('fsrsParams', params)} onOptimised={(r) => void onOptimised(r)} />
                    ))}
                  </Card>
                </Section>
              ))}
            </>
          )}
          {dirty && (
            <div ref={saveBarRef} className={ps.saveBar} role="region" aria-label={t('preset.unsaved')}>
              <span className={ps.saveText}>{t('preset.unsaved')}</span>
              <Button variant="ghost" className={ps.saveGhost} onClick={() => { if (q.data) { setDraft(q.data.config); setName(q.data.name); } }}>{t('preset.discard')}</Button>
              <Button variant="primary" loading={saving} kbd="ctrl+s" onClick={save}>{t('common.save')}</Button>
            </div>
          )}
        </div>
      </div>
      <Dialog
        open={confirmLeave !== null}
        onClose={() => setConfirmLeave(null)}
        title={t('preset.leaveTitle')}
        description={t('preset.leaveBody')}
        destructive
        confirmLabel={t('preset.discard')}
        onConfirm={() => { const href = confirmLeave!; setConfirmLeave(null); if (q.data) { setDraft(q.data.config); setName(q.data.name); } setTimeout(() => navigate(href), 0); }}
      />
    </Page>
  );
}

function FieldRow({ field, config, set, presetId, onParams, onOptimised }: {
  field: PresetField; config: DeckConfig; set: <K extends keyof DeckConfig>(k: K, v: DeckConfig[K]) => void; presetId: number;
  onParams: (p: number[]) => void; onOptimised: (r: OptimiseResult) => void;
}) {
  const key = field.key;
  const value = config[key];
  const rec = isRecommended(key, value);
  const inactive = !!field.only && field.only !== config.scheduler;
  const label = t(`preset.f.${key}`);
  const extra: ReactNode = (
    <>
      {rec && <span className={ps.chip}>{t('preset.recommended')}</span>}
      {field.only && <span className={ps.only}>{field.only === 'sm2' ? t('preset.onlySm2') : t('preset.onlyFsrs')}</span>}
      {!rec && field.kind.type !== 'params' && (
        <button type="button" className={ps.useRec} onClick={() => set(key, STANDARD[key] as never)} aria-label={t('preset.useRecommendedLabel', { field: label, value: displayValue(field, STANDARD[key]) })}>
          {t('preset.useRecommended', { value: displayValue(field, STANDARD[key]) })}
        </button>
      )}
    </>
  );
  const k = field.kind;
  let control: ReactNode;
  let stack = false;
  let inline = false;
  switch (k.type) {
    case 'bool':
      inline = true;
      control = <Switch label={label} checked={value as boolean} onChange={(v) => set(key, v as never)} />;
      break;
    case 'int':
      control = <NumberInput label={label} value={value as number} min={k.min} max={k.max} step={1} unit={k.unit ? t(`unit.${k.unit}`) : undefined} onChange={(v) => set(key, v as never)} />;
      break;
    case 'float':
      control = <NumberInput label={label} value={value as number} min={k.min} max={k.max} step={k.step} unit={k.unit} onChange={(v) => set(key, v as never)} />;
      break;
    case 'permille':
      control = <NumberInput label={label} value={(value as number) / 10} min={k.min / 10} max={k.max / 10} step={5} unit="%" onChange={(v) => set(key, Math.round(v * 10) as never)} />;
      break;
    case 'percent':
      stack = true;
      control = <Slider label={label} value={value as number} min={k.min} max={k.max} step={k.step} format={(v) => formatPercent(v)} showScale onChange={(v) => set(key, v as never)} />;
      // desired retention: the same workload sentence and sparkline as Settings › Studying
      if (key === 'desiredRetention') control = <div className={s.retention}>{control}<RetentionForecast config={config} presetKey={presetId} /></div>;
      break;
    case 'select':
      control = <Select className={s.selectControl} aria-label={label} value={value as string} onChange={(v) => set(key, v as never)} options={k.options.map((o) => ({ value: o, label: t(`preset.opt.${key}.${o}`) }))} />;
      break;
    case 'steps':
      control = <StepsInput label={label} value={value as number[]} onChange={(v) => set(key, v as never)} />;
      break;
    case 'params':
      stack = true;
      control = <ParamsEditor presetId={presetId} params={value as number[]} onChange={onParams} onOptimised={onOptimised} />;
      break;
  }
  return (
    <div className={cx(inactive && ps.inactive)} id={`field-${key}`}>
      <SettingRow label={<span className={ps.rowLabel}>{label}</span>} help={t(`preset.h.${key}`)} extra={extra} stack={stack} inline={inline}>
        {control}
      </SettingRow>
    </div>
  );
}

function NumberInput({ label, value, min, max, step, unit, onChange }: { label: string; value: number; min: number; max: number; step: number; unit?: string; onChange: (v: number) => void }) {
  const [text, setText] = useState(String(value));
  useEffect(() => { setText(String(value)); }, [value]);
  const commit = () => {
    const n = Number(text);
    if (!Number.isFinite(n) || text.trim() === '') { setText(String(value)); return; }
    const v = Math.max(min, Math.min(max, step >= 1 ? Math.round(n) : n));
    setText(String(v));
    if (v !== value) onChange(v);
  };
  return (
    <Input
      className={ps.number}
      type="number" inputMode={step >= 1 ? 'numeric' : 'decimal'}
      aria-label={label} min={min} max={max} step={step} suffix={unit}
      value={text}
      onChange={(e) => setText(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => { if (e.key === 'Enter') commit(); }}
    />
  );
}

function StepsInput({ label, value, onChange }: { label: string; value: number[]; onChange: (v: number[]) => void }) {
  const [text, setText] = useState(formatSteps(value));
  const [error, setError] = useState(false);
  useEffect(() => { setText(formatSteps(value)); setError(false); }, [value]);
  const commit = () => {
    const parsed = parseSteps(text);
    if (!parsed) { setError(true); return; }
    setError(false);
    if (formatSteps(parsed) !== formatSteps(value)) onChange(parsed);
  };
  return (
    <Input
      className={ps.stepsInput} mono aria-label={label} aria-invalid={error || undefined}
      title={error ? t('preset.stepsInvalid') : undefined}
      value={text} onChange={(e) => setText(e.target.value)} onBlur={commit} onKeyDown={(e) => { if (e.key === 'Enter') commit(); }}
      placeholder="1m 10m"
    />
  );
}

function ParamsEditor({ presetId, params, onChange, onOptimised }: { presetId: number; params: number[]; onChange: (p: number[]) => void; onOptimised: (r: OptimiseResult) => void }) {
  const setManualOptimise = useUi((st) => st.setManualOptimise);
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<{ phase: string; value: number } | null>(null);
  const [result, setResult] = useState<OptimiseResult | null>(null);
  const [evalResult, setEvalResult] = useState<{ logLoss: number; rmse: number; reviews: number } | null>(null);
  const [evaluating, setEvaluating] = useState(false);
  const before = result?.before.params;

  const optimise = async () => {
    setProgress({ phase: t('preset.optimising'), value: 0 });
    setResult(null);
    setManualOptimise(presetId, true); // this view says what happened; the shell's generic toast stays quiet
    try {
      const r = await api.optimise(presetId, (p) => setProgress({ phase: p.phase, value: p.total ? p.done / p.total : 0 }));
      setResult(r);
      // The collection already stored adopted parameters (undoable); the parent reloads them.
      onOptimised(r);
    } catch (e) {
      toast({ message: t('preset.optimiseFailed', { error: errorMessage(e) }), tone: 'danger' });
    } finally {
      setProgress(null);
      window.setTimeout(() => setManualOptimise(presetId, false), 1000);
    }
  };
  const evaluate = async () => {
    setEvaluating(true);
    try { setEvalResult(await api.evaluate(presetId)); }
    catch (e) { toast({ message: errorMessage(e), tone: 'danger' }); }
    finally { setEvaluating(false); }
  };

  return (
    <div className={ps.paramsBlock}>
      {editing ? (
        <Field label={t('preset.paramsPaste')} help={t('preset.paramsPasteHelp')} error={error}>
          <Textarea mono rows={3} value={text} onChange={(e) => setText(e.target.value)} data-autofocus />
        </Field>
      ) : (
        <div className={ps.params} role="list" aria-label={t('preset.f.fsrsParams')}>
          {params.map((p, i) => (
            <div role="listitem" key={i} className={cx(ps.param, before && Math.abs((before[i] ?? p) - p) > 1e-9 && ps.changed)}>
              <span className={ps.paramKey}>w{i}</span>
              <span className={ps.paramVal}>{p.toFixed(4)}</span>
            </div>
          ))}
        </div>
      )}
      {progress && <ProgressBar value={progress.value} label={progress.phase} />}
      {result && (
        <div className={ps.result} role="status">
          <div className={ps.metrics}>
            <span className={ps.metric}><span className={ps.metricLabel}>{t('preset.logLoss')}</span><span className={ps.metricValue}>{result.before.logLoss.toFixed(4)}<span className={ps.arrow}>→</span>{result.after.logLoss.toFixed(4)}</span></span>
            <span className={ps.metric}><span className={ps.metricLabel}>{t('preset.rmse')}</span><span className={ps.metricValue}>{formatPercent(result.before.rmse, 1)}<span className={ps.arrow}>→</span>{formatPercent(result.after.rmse, 1)}</span></span>
            <span className={ps.metric}><span className={ps.metricLabel}>{t('preset.reviews')}</span><span className={ps.metricValue}>{formatCount(result.reviews)}</span></span>
          </div>
          <span>{result.adopted ? t('preset.adoptedNote') : result.reason}</span>
        </div>
      )}
      {evalResult && !result && (
        <div className={ps.result} role="status">
          <div className={ps.metrics}>
            <span className={ps.metric}><span className={ps.metricLabel}>{t('preset.logLoss')}</span><span className={ps.metricValue}>{evalResult.logLoss.toFixed(4)}</span></span>
            <span className={ps.metric}><span className={ps.metricLabel}>{t('preset.rmse')}</span><span className={ps.metricValue}>{formatPercent(evalResult.rmse, 1)}</span></span>
            <span className={ps.metric}><span className={ps.metricLabel}>{t('preset.reviews')}</span><span className={ps.metricValue}>{formatCount(evalResult.reviews)}</span></span>
          </div>
          <span>{t('preset.evalNote')}</span>
        </div>
      )}
      <div className={ps.paramActions}>
        {editing ? (
          <>
            <Button size="sm" variant="primary" onClick={() => { const p = parseParams(text); if (!p) { setError(t('preset.paramsInvalid')); return; } onChange(p); setEditing(false); setError(null); }}>{t('preset.paramsApply')}</Button>
            <Button size="sm" variant="ghost" onClick={() => { setEditing(false); setError(null); }}>{t('common.cancel')}</Button>
          </>
        ) : (
          <>
            <Button size="sm" variant="primary" icon="sparkle" loading={!!progress} onClick={optimise}>{t('preset.optimise')}</Button>
            <Button size="sm" variant="secondary" icon="target" loading={evaluating} onClick={evaluate}>{t('preset.evaluate')}</Button>
            <Button size="sm" variant="ghost" icon="edit" onClick={() => { setText(params.join(', ')); setEditing(true); }}>{t('preset.paramsEdit')}</Button>
            {!isRecommended('fsrsParams', params) && <Button size="sm" variant="ghost" icon="undo" onClick={() => onChange(FSRS6_DEFAULT)}>{t('preset.paramsReset')}</Button>}
          </>
        )}
      </div>
    </div>
  );
}

export type { DeckConfigPreset };
