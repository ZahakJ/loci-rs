import { useEffect, useState } from 'react';
import { t } from '@core/i18n/i18n';
import type { DeckConfigPreset, DeckTreeNode } from '@core/types';
import { api, errorMessage } from '../../api';
import { Button, IconButton } from '../../design/primitives/Button';
import { Dialog } from '../../design/primitives/Dialog';
import { Field } from '../../design/primitives/Field';
import { Input } from '../../design/primitives/Input';
import { Menu } from '../../design/primitives/Menu';
import { Pill } from '../../design/primitives/Pill';
import { Select } from '../../design/primitives/Select';
import { Skeleton } from '../../design/primitives/Skeleton';
import { toast } from '../../design/primitives/Toast';
import { formatPercent } from '../../hooks/format';
import { navigate } from '../../routes';
import { STANDARD } from './presetFields';
import s from './Settings.module.css';

type DialogState =
  | null
  | { kind: 'add' }
  | { kind: 'rename' | 'duplicate' | 'delete' | 'assign'; preset: DeckConfigPreset };

function flatten(tree: DeckTreeNode | undefined): DeckTreeNode[] {
  const out: DeckTreeNode[] = [];
  const walk = (n: DeckTreeNode) => { if (n.level > 0) out.push(n); n.children.forEach(walk); };
  if (tree) walk(tree);
  return out;
}

/** Preset list: add, rename, duplicate, delete, assign to deck. */
export function PresetList({ presets, tree, onChanged }: { presets: DeckConfigPreset[] | undefined; tree: DeckTreeNode | undefined; onChanged: () => void }) {
  const [dialog, setDialog] = useState<DialogState>(null);
  const [name, setName] = useState('');
  const [deckId, setDeckId] = useState<number>(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const decks = flatten(tree);

  useEffect(() => {
    setError(null); setBusy(false);
    if (!dialog) return;
    if (dialog.kind === 'add') setName('');
    if (dialog.kind === 'rename') setName(dialog.preset.name);
    if (dialog.kind === 'duplicate') setName(t('presets.copyName', { name: dialog.preset.name }));
    if (dialog.kind === 'assign') setDeckId(decks.find((d) => d.deck.configId !== dialog.preset.id)?.deck.id ?? decks[0]?.deck.id ?? 0);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dialog]);

  const run = async (fn: () => Promise<unknown>, done?: string) => {
    setBusy(true); setError(null);
    try { await fn(); setDialog(null); onChanged(); if (done) toast({ message: done, icon: 'check', tone: 'success' }); }
    catch (e) { setError(errorMessage(e)); setBusy(false); }
  };

  const using = (id: number) => decks.filter((d) => d.deck.configId === id);
  const p = dialog && dialog.kind !== 'add' ? dialog.preset : null;

  return (
    <div>
      <div className={s.subhead}>{t('presets.title')}</div>
      {!presets ? <div className={s.row}><Skeleton lines={3} /></div> : presets.map((preset) => {
        const used = using(preset.id);
        return (
          <div key={preset.id} className={s.presetRow}>
            <div className={s.presetMain}>
              <a className={s.presetName} href={`#/settings/presets/${preset.id}`}>{preset.name}</a>
              <span className={s.presetMeta}>
                {used.length ? t('settings.usedBy', { count: used.length, n: used.length }) : t('settings.usedByNone')}
                {' · '}{t('presets.summary', { r: formatPercent(preset.config.desiredRetention), n: preset.config.newPerDay, m: preset.config.minutesPerDay })}
              </span>
            </div>
            {preset.id === 1 && <Pill tone="accent">{t('presets.default')}</Pill>}
            <Menu
              label={t('presets.menu', { name: preset.name })}
              items={[
                { id: 'edit', label: t('presets.edit'), icon: 'edit', onSelect: () => navigate(`#/settings/presets/${preset.id}`) },
                { id: 'rename', label: t('presets.rename'), icon: 'text', onSelect: () => setDialog({ kind: 'rename', preset }) },
                { id: 'duplicate', label: t('presets.duplicate'), icon: 'copy', onSelect: () => setDialog({ kind: 'duplicate', preset }) },
                { id: 'assign', label: t('presets.assign'), icon: 'cards', onSelect: () => setDialog({ kind: 'assign', preset }), disabled: decks.length === 0 },
                { separator: true, id: 's' },
                { id: 'delete', label: t('presets.delete'), icon: 'trash', danger: true, disabled: preset.id === 1, onSelect: () => setDialog({ kind: 'delete', preset }) },
              ]}
              trigger={(tp) => <IconButton {...tp} size="sm" icon="more" label={t('presets.menu', { name: preset.name })} />}
            />
          </div>
        );
      })}
      <div className={s.presetFoot}>
        <Button variant="ghost" icon="plus" onClick={() => setDialog({ kind: 'add' })}>{t('presets.add')}</Button>
      </div>

      <Dialog
        open={dialog?.kind === 'add' || dialog?.kind === 'rename' || dialog?.kind === 'duplicate'}
        onClose={() => setDialog(null)}
        title={dialog?.kind === 'add' ? t('presets.addTitle') : dialog?.kind === 'rename' ? t('presets.renameTitle') : t('presets.duplicateTitle')}
        icon={dialog?.kind === 'duplicate' ? 'copy' : 'settings'}
        description={dialog?.kind === 'add' ? t('presets.addHelp') : undefined}
        confirmLabel={dialog?.kind === 'rename' ? t('common.save') : t('presets.create')}
        busy={busy}
        confirmDisabled={!name.trim()}
        onConfirm={() => run(async () => {
          if (dialog?.kind === 'add') await api.savePreset({ id: 0, name: name.trim(), config: STANDARD });
          else if (p && dialog?.kind === 'rename') await api.savePreset({ id: p.id, name: name.trim(), config: p.config });
          else if (p) await api.savePreset({ id: 0, name: name.trim(), config: p.config });
        })}
      >
        <Field label={t('presets.name')} error={error}>
          <Input value={name} onChange={(e) => setName(e.target.value)} data-autofocus autoComplete="off" />
        </Field>
      </Dialog>

      <Dialog
        open={dialog?.kind === 'assign'}
        onClose={() => setDialog(null)}
        title={t('presets.assignTitle', { name: p?.name ?? '' })}
        icon="cards"
        confirmLabel={t('presets.assignConfirm')}
        busy={busy}
        confirmDisabled={!deckId}
        onConfirm={() => run(() => api.setDeckPreset(deckId, p!.id), t('presets.assigned'))}
      >
        <Field label={t('presets.deck')} help={t('presets.assignHelp')} error={error}>
          <Select
            value={deckId}
            onChange={setDeckId}
            data-autofocus
            options={decks.map((d) => ({ value: d.deck.id, label: `${' '.repeat(d.level - 1)}${d.deck.name.split('::').pop()}${d.deck.configId === p?.id ? ` (${t('presets.current')})` : ''}` }))}
          />
        </Field>
      </Dialog>

      <Dialog
        open={dialog?.kind === 'delete'}
        onClose={() => setDialog(null)}
        destructive
        title={t('presets.deleteTitle', { name: p?.name ?? '' })}
        description={t('presets.deleteBody', { count: p ? using(p.id).length : 0, n: p ? using(p.id).length : 0 })}
        confirmLabel={t('presets.delete')}
        busy={busy}
        onConfirm={() => run(() => api.deletePreset(p!.id), t('presets.deleted'))}
      >
        {error && <p className={s.danger} role="alert">{error}</p>}
      </Dialog>
    </div>
  );
}
