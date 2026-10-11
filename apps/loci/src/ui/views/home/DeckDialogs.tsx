import { useEffect, useState } from 'react';
import { t } from '@core/i18n/i18n';
import type { DeckConfigPreset, DeckTreeNode } from '@core/types';
import { api, errorMessage } from '../../api';
import { Dialog } from '../../design/primitives/Dialog';
import { Field } from '../../design/primitives/Field';
import { Input, Textarea } from '../../design/primitives/Input';
import { Select } from '../../design/primitives/Select';
import { LinkButton } from '../../design/primitives/Button';
import { toast } from '../../design/primitives/Toast';
import { formatCount, formatPercent } from '../../hooks/format';
import { runUndo } from '../../keys/useGlobalKeys';
import s from './DeckDialogs.module.css';

export type DeckDialogState =
  | null
  | { kind: 'create' }
  | { kind: 'rename' | 'options' | 'description' | 'delete'; node: DeckTreeNode };

const leaf = (name: string) => name.split('::').pop() ?? name;

/** All deck dialogs (create, rename, options, description, delete) driven by one state. */
export function DeckDialogs({ state, onClose }: { state: DeckDialogState; onClose: () => void }) {
  const [text, setText] = useState('');
  const [presetId, setPresetId] = useState<number>(1);
  const [presets, setPresets] = useState<DeckConfigPreset[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setError(null);
    setBusy(false);
    if (!state) return;
    if (state.kind === 'create') setText('');
    else if (state.kind === 'rename') setText(state.node.deck.name);
    else if (state.kind === 'description') setText(state.node.deck.description);
    else if (state.kind === 'options') {
      setPresetId(state.node.deck.configId);
      void api.listPresets().then(setPresets);
    }
  }, [state]);

  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try { await fn(); onClose(); }
    catch (e) { setError(errorMessage(e)); setBusy(false); }
  };

  const kind = state?.kind;
  const node = state && state.kind !== 'create' ? state.node : null;
  const name = node ? leaf(node.deck.name) : '';

  const preset = presets.find((p) => p.id === presetId);
  return (
    <>
      <Dialog
        open={kind === 'create'}
        onClose={onClose}
        title={t('deck.newTitle')}
        icon="plus"
        confirmLabel={t('deck.create')}
        busy={busy}
        confirmDisabled={!text.trim()}
        onConfirm={() => run(async () => {
          const d = await api.createDeck(text.trim());
          toast({ message: t('deck.created', { name: leaf(d.name) }), icon: 'check', tone: 'success' });
        })}
      >
        <Field label={t('deck.name')} help={t('deck.nameHelp')} error={error}>
          <Input value={text} onChange={(e) => setText(e.target.value)} placeholder={t('deck.namePlaceholder')} data-autofocus autoComplete="off" />
        </Field>
      </Dialog>

      <Dialog
        open={kind === 'rename'}
        onClose={onClose}
        title={t('deck.renameTitle', { name })}
        icon="edit"
        confirmLabel={t('common.save')}
        busy={busy}
        confirmDisabled={!text.trim() || text.trim() === node?.deck.name}
        onConfirm={() => run(() => api.renameDeck(node!.deck.id, text.trim()))}
      >
        <Field label={t('deck.name')} help={t('deck.nameHelp')} error={error}>
          <Input value={text} onChange={(e) => setText(e.target.value)} data-autofocus autoComplete="off" onFocus={(e) => e.target.select()} />
        </Field>
      </Dialog>

      <Dialog
        open={kind === 'description'}
        onClose={onClose}
        title={t('deck.descriptionTitle', { name })}
        icon="text"
        confirmLabel={t('common.save')}
        busy={busy}
        onConfirm={() => run(() => api.setDeckDescription(node!.deck.id, text))}
      >
        <Field label={t('deck.description')} help={t('deck.descriptionHelp')} error={error}>
          <Textarea value={text} onChange={(e) => setText(e.target.value)} rows={4} data-autofocus />
        </Field>
      </Dialog>

      <Dialog
        open={kind === 'options'}
        onClose={onClose}
        title={t('deck.optionsTitle', { name })}
        icon="settings"
        confirmLabel={t('common.save')}
        busy={busy}
        onConfirm={() => run(() => api.setDeckPreset(node!.deck.id, presetId))}
      >
        <div className={s.stack}>
          <Field label={t('deck.preset')} help={t('deck.presetHelp')} error={error}>
            <Select value={presetId} onChange={setPresetId} options={presets.map((p) => ({ value: p.id, label: p.name }))} data-autofocus />
          </Field>
          {preset && (
            <dl className={s.summary}>
              <div><dt>{t('preset.summary.new')}</dt><dd>{formatCount(preset.config.newPerDay)}</dd></div>
              <div><dt>{t('preset.summary.retention')}</dt><dd>{formatPercent(preset.config.desiredRetention)}</dd></div>
              <div><dt>{t('preset.summary.budget')}</dt><dd>{t('time.short', { m: preset.config.minutesPerDay })}</dd></div>
            </dl>
          )}
          <LinkButton href={`#/settings/presets/${presetId}`} variant="ghost" size="sm" iconEnd="chevron-right" className={s.edit} onClick={onClose}>
            {t('deck.editPreset')}
          </LinkButton>
        </div>
      </Dialog>

      <Dialog
        open={kind === 'delete'}
        onClose={onClose}
        destructive
        title={t('deck.deleteTitle', { name })}
        description={t('deck.deleteBody', { count: node?.totalCards ?? 0, n: formatCount(node?.totalCards ?? 0) })}
        confirmLabel={t('deck.deleteConfirm')}
        busy={busy}
        onConfirm={() => run(async () => {
          await api.deleteDeck(node!.deck.id);
          toast({ message: t('deck.deleted', { name }), icon: 'trash', action: { label: t('common.undo'), onClick: () => void runUndo() } });
        })}
      >
        {error && <p className={s.error} role="alert">{error}</p>}
      </Dialog>
    </>
  );
}
