/**
 * Bulk actions for the selection (DESIGN §11). Each one goes through `api.bulk` (undoable) and
 * reports with a toast carrying Undo. Destructive or parameterised actions ask first.
 */
import { useState } from 'react';
import { t } from '@core/i18n/i18n';
import type { BulkAction, Flag } from '@core/types';
import { Button, IconButton } from '@ui/design/primitives/Button';
import { Dialog } from '@ui/design/primitives/Dialog';
import { Field } from '@ui/design/primitives/Field';
import { Input } from '@ui/design/primitives/Input';
import { Tooltip } from '@ui/design/primitives/Tooltip';
import { Menu } from '@ui/design/primitives/Menu';
import { Select } from '@ui/design/primitives/Select';
import { Switch } from '@ui/design/primitives/Switch';
import { cx } from '@ui/design/primitives/layer';
import { Combobox, deckOptions } from '../edit/Combobox';
import { TagInput } from '../edit/TagInput';
import s from './BulkBar.module.css';

export interface BulkBarProps {
  count: number;
  noun: 'cards' | 'notes';
  onRun: (action: BulkAction) => Promise<void>;
  onClear: () => void;
  decks: Array<{ id: number; name: string }>;
  fieldNames: string[];
  /** card info for the single selected card (absent when several are selected) */
  onInfo?: () => void;
  className?: string;
  /** controlled dialog (keyboard shortcuts open them from the view) */
  dialog?: DialogKind | null;
  onDialogChange?: (d: DialogKind | null) => void;
}

export type DialogKind = 'deck' | 'addTags' | 'removeTags' | 'reposition' | 'setDue' | 'forget' | 'delete' | 'findReplace';

export const FLAG_COLOURS: Flag[] = [1, 2, 3, 4, 5, 6, 7];

export function BulkBar({ count, noun, onRun, onClear, decks, fieldNames, onInfo, className, dialog, onDialogChange }: BulkBarProps) {
  const [ownOpen, setOwnOpen] = useState<DialogKind | null>(null);
  const open = dialog !== undefined ? dialog : ownOpen;
  const setOpen = (d: DialogKind | null) => { setOwnOpen(d); onDialogChange?.(d); };
  const [busy, setBusy] = useState(false);
  const [deckId, setDeckId] = useState<number | null>(null);
  const [tags, setTags] = useState<string[]>([]);
  const [repo, setRepo] = useState({ start: '0', step: '1', randomize: false, shift: false });
  const [due, setDue] = useState('0');
  const [forget, setForget] = useState({ resetCounts: false, restorePosition: true });
  const [fr, setFr] = useState({ find: '', replace: '', regex: false, matchCase: false, field: '' });

  const run = async (a: BulkAction) => {
    setBusy(true);
    try { await onRun(a); setOpen(null); } finally { setBusy(false); }
  };
  const openDialog = (k: DialogKind) => { setTags([]); setOpen(k); };

  const regexError = (() => {
    if (!fr.regex || !fr.find) return null;
    try { new RegExp(fr.find); return null; } catch (e) { return (e as Error).message; }
  })();

  return (
    <div className={cx(s.bar, className)} role="toolbar" aria-label={t('browse.bulk.label')}>
      <span className={s.count}>
        <IconButton size="sm" icon="close" label={t('browse.clearSelection')} onClick={onClear} />
        <span aria-live="polite">{t(noun === 'notes' ? 'browse.selectedNotes' : 'browse.selectedCards', { count })}</span>
      </span>
      <span className={s.sep} aria-hidden />
      <div className={s.actions}>
        <Tooltip content={t('browse.bulk.changeDeck')}><Button size="sm" variant="ghost" icon="deck" onClick={() => openDialog('deck')} className={s.act} aria-label={t('browse.bulk.changeDeck')}>{t('browse.bulk.changeDeck')}</Button></Tooltip>
        <Tooltip content={t('browse.bulk.addTags')}><Button size="sm" variant="ghost" icon="tag" onClick={() => openDialog('addTags')} className={s.act} aria-label={t('browse.bulk.addTags')}>{t('browse.bulk.addTags')}</Button></Tooltip>
        <Tooltip content={t('browse.bulk.suspend')}><Button size="sm" variant="ghost" icon="suspend" onClick={() => void run({ kind: 'suspend' })} className={s.act} aria-label={t('browse.bulk.suspend')}>{t('browse.bulk.suspend')}</Button></Tooltip>
        <Menu
          label={t('browse.bulk.flag')}
          align="start"
          items={[
            ...FLAG_COLOURS.map((f) => ({ id: `f${f}`, label: t(`study.flags.${f}`), icon: 'flag' as const, kbd: `ctrl+${f}`, onSelect: () => void run({ kind: 'flag', flag: f }) })),
            { separator: true as const, id: 'sep' },
            { id: 'f0', label: t('study.flags.0'), onSelect: () => void run({ kind: 'flag', flag: 0 }) },
          ]}
          trigger={(p) => <Tooltip content={t('browse.bulk.flag')}><Button {...p} size="sm" variant="ghost" icon="flag" iconEnd="chevron-down" className={s.act} aria-label={t('browse.bulk.flag')}>{t('browse.bulk.flag')}</Button></Tooltip>}
        />
        <Menu
          label={t('browse.bulk.more')}
          align="end"
          items={[
            ...(onInfo ? [
              { id: 'info', label: t('browse.cardInfo'), icon: 'info' as const, kbd: 'i', onSelect: onInfo },
              { separator: true as const, id: 's0' },
            ] : []),
            { id: 'rmtags', label: t('browse.bulk.removeTags'), icon: 'tag', onSelect: () => openDialog('removeTags') },
            { id: 'unsuspend', label: t('browse.bulk.unsuspend'), icon: 'play', onSelect: () => void run({ kind: 'unsuspend' }) },
            { id: 'bury', label: t('browse.bulk.bury'), icon: 'bury', onSelect: () => void run({ kind: 'bury' }) },
            { id: 'unbury', label: t('browse.bulk.unbury'), icon: 'sun', onSelect: () => void run({ kind: 'unbury' }) },
            { separator: true, id: 's1' },
            { id: 'repo', label: t('browse.bulk.reposition'), icon: 'reposition', onSelect: () => openDialog('reposition') },
            { id: 'due', label: t('browse.bulk.setDue'), icon: 'calendar', kbd: 'ctrl+shift+d', onSelect: () => openDialog('setDue') },
            { id: 'forget', label: t('browse.bulk.forget'), icon: 'forget', kbd: 'ctrl+alt+n', onSelect: () => openDialog('forget') },
            { id: 'fr', label: t('browse.bulk.findReplace'), icon: 'replace', kbd: 'ctrl+alt+f', onSelect: () => openDialog('findReplace') },
            // Deferred (docs/DEFERRED.md): listed so it can be found, but not offered yet.
            { id: 'chnt', label: t('browse.changeNotetypeLater'), icon: 'cards', disabled: true, onSelect: () => undefined },
            { separator: true, id: 's2' },
            { id: 'delete', label: t('browse.bulk.delete'), icon: 'trash', danger: true, kbd: 'ctrl+delete', onSelect: () => openDialog('delete') },
          ]}
          trigger={(p) => <Tooltip content={t('browse.bulk.more')}><Button {...p} size="sm" variant="ghost" icon="more" className={s.act} aria-label={t('browse.bulk.more')}>{t('browse.bulk.more')}</Button></Tooltip>}
        />
      </div>
      <Tooltip content={t('browse.bulk.delete')}><Button size="sm" variant="quiet-danger" icon="trash" onClick={() => openDialog('delete')} className={cx(s.act, s.delete)} aria-label={t('browse.bulk.delete')}>{t('browse.bulk.delete')}</Button></Tooltip>

      <Dialog open={open === 'deck'} onClose={() => setOpen(null)} title={t('browse.bulk.changeDeck')} description={t('browse.bulk.changeDeckHelp', { count })}
        confirmLabel={t('browse.bulk.move')} busy={busy} confirmDisabled={deckId === null} onConfirm={async () => { if (deckId !== null) await run({ kind: 'changeDeck', deckId }); }}>
        <Combobox label={t('editor.deck')} value={deckId} options={deckOptions(decks)} onChange={setDeckId} icon="deck" placeholder={t('editor.chooseDeck')} />
      </Dialog>

      <Dialog open={open === 'addTags' || open === 'removeTags'} onClose={() => setOpen(null)}
        title={open === 'removeTags' ? t('browse.bulk.removeTags') : t('browse.bulk.addTags')}
        confirmLabel={open === 'removeTags' ? t('browse.bulk.remove') : t('browse.bulk.add')} busy={busy} confirmDisabled={!tags.length}
        onConfirm={() => run(open === 'removeTags' ? { kind: 'removeTags', tags } : { kind: 'addTags', tags })}>
        <TagInput label={t('editor.tags')} tags={tags} onChange={setTags} placeholder={t('editor.tagsPlaceholder')} autoFocus />
      </Dialog>

      <Dialog open={open === 'reposition'} onClose={() => setOpen(null)} title={t('browse.bulk.reposition')} description={t('browse.bulk.repositionHelp')}
        confirmLabel={t('browse.bulk.reposition')} busy={busy}
        confirmDisabled={!/^\d+$/.test(repo.start) || !/^[1-9]\d*$/.test(repo.step)}
        onConfirm={() => run({ kind: 'reposition', start: Number(repo.start), step: Number(repo.step), randomize: repo.randomize, shift: repo.shift })}>
        <div className={s.form}>
          <div className={s.two}>
            <Field label={t('browse.bulk.start')}><Input inputMode="numeric" value={repo.start} onChange={(e) => setRepo({ ...repo, start: e.target.value })} data-autofocus /></Field>
            <Field label={t('browse.bulk.step')}><Input inputMode="numeric" value={repo.step} onChange={(e) => setRepo({ ...repo, step: e.target.value })} /></Field>
          </div>
          <Field label={t('browse.bulk.randomize')} inline><Switch checked={repo.randomize} onChange={(v) => setRepo({ ...repo, randomize: v })} /></Field>
          <Field label={t('browse.bulk.shift')} inline><Switch checked={repo.shift} onChange={(v) => setRepo({ ...repo, shift: v })} /></Field>
        </div>
      </Dialog>

      <Dialog open={open === 'setDue'} onClose={() => setOpen(null)} title={t('browse.bulk.setDue')}
        confirmLabel={t('browse.bulk.setDue')} busy={busy} confirmDisabled={!/^\d+(-\d+)?!?$/.test(due.trim())}
        onConfirm={() => run({ kind: 'setDue', days: due.trim() })}>
        <Field label={t('browse.bulk.days')} help={t('browse.bulk.setDueHelp')}>
          <Input mono value={due} onChange={(e) => setDue(e.target.value)} data-autofocus />
        </Field>
      </Dialog>

      <Dialog open={open === 'forget'} onClose={() => setOpen(null)} title={t('browse.bulk.forget')} description={t('browse.bulk.forgetHelp', { count })}
        confirmLabel={t('browse.bulk.forget')} busy={busy} onConfirm={() => run({ kind: 'forget', ...forget })}>
        <div className={s.form}>
          <Field label={t('browse.bulk.forgetRestorePosition')} inline><Switch checked={forget.restorePosition} onChange={(v) => setForget({ ...forget, restorePosition: v })} /></Field>
          <Field label={t('browse.bulk.forgetResetCounts')} inline><Switch checked={forget.resetCounts} onChange={(v) => setForget({ ...forget, resetCounts: v })} /></Field>
        </div>
      </Dialog>

      <Dialog open={open === 'delete'} onClose={() => setOpen(null)} destructive title={t('browse.bulk.deleteTitle')}
        description={noun === 'notes' ? t('browse.bulk.deleteConfirm', { count }) : t('browse.bulk.deleteCardsConfirm', { count })}
        confirmLabel={t('browse.bulk.delete')} busy={busy} onConfirm={() => run({ kind: 'delete' })} />

      <Dialog open={open === 'findReplace'} onClose={() => setOpen(null)} title={t('browse.bulk.findReplace')} description={t('browse.bulk.findReplaceHelp', { count })}
        confirmLabel={t('browse.bulk.replaceAll')} busy={busy} confirmDisabled={!fr.find || !!regexError} width="32rem"
        onConfirm={() => run({ kind: 'findReplace', find: fr.find, replace: fr.replace, regex: fr.regex, matchCase: fr.matchCase, field: fr.field || null })}>
        <div className={s.form}>
          <Field label={t('browse.bulk.find')} error={regexError ?? undefined}><Input mono value={fr.find} onChange={(e) => setFr({ ...fr, find: e.target.value })} data-autofocus /></Field>
          <Field label={t('browse.bulk.replaceWith')}><Input mono value={fr.replace} onChange={(e) => setFr({ ...fr, replace: e.target.value })} /></Field>
          <Field label={t('browse.bulk.inField')}>
            <Select value={fr.field} onChange={(v) => setFr({ ...fr, field: v })} options={[{ value: '', label: t('browse.bulk.allFields') }, ...fieldNames.map((n) => ({ value: n, label: n }))]} />
          </Field>
          <Field label={t('browse.bulk.regex')} inline><Switch checked={fr.regex} onChange={(v) => setFr({ ...fr, regex: v })} /></Field>
          <Field label={t('browse.bulk.matchCase')} inline><Switch checked={fr.matchCase} onChange={(v) => setFr({ ...fr, matchCase: v })} /></Field>
        </div>
      </Dialog>
    </div>
  );
}
