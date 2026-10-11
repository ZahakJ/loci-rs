/**
 * Fields of a note type: add, rename (template references follow), reorder (buttons and
 * Alt+Arrow keys), remove, and options: sticky, right-to-left, plain text, placeholder, sort field.
 */
import { useState } from 'react';
import { t } from '@core/i18n/i18n';
import type { Field, Notetype } from '@core/types';
import { Icon } from '@ui/design/icons';
import { Button, IconButton } from '@ui/design/primitives/Button';
import { Dialog } from '@ui/design/primitives/Dialog';
import { Field as FormField } from '@ui/design/primitives/Field';
import { Input } from '@ui/design/primitives/Input';
import { Switch } from '@ui/design/primitives/Switch';
import { cx } from '@ui/design/primitives/layer';
import { moveItem, renameFieldRefs, uniqueName } from './templateText';
import s from './Notetypes.module.css';

export interface FieldsEditorProps {
  draft: Notetype;
  onChange: (nt: Notetype) => void;
  onAnnounce: (msg: string) => void;
}

const defaultConfig: Field['config'] = { sticky: false, rtl: false, font: '', size: 0, description: '', plainText: false, collapsed: false };

export function FieldsEditor({ draft, onChange, onAnnounce }: FieldsEditorProps) {
  const [open, setOpen] = useState<number | null>(null);
  const [newName, setNewName] = useState('');
  const [removing, setRemoving] = useState<number | null>(null);
  const names = draft.fields.map((f) => f.name);

  const setFields = (fields: Field[], extra: Partial<Notetype> = {}) => onChange({ ...draft, ...extra, fields });
  const update = (i: number, patch: Partial<Field['config']>) => setFields(draft.fields.map((f, k) => (k === i ? { ...f, config: { ...f.config, ...patch } } : f)));
  const rename = (i: number, name: string) => {
    const old = draft.fields[i]!.name;
    const clean = name.replace(/[:{}"]/g, '').trimStart();
    setFields(draft.fields.map((f, k) => (k === i ? { ...f, name: clean } : f)), {
      templates: draft.templates.map((tm) => ({ ...tm, qfmt: renameFieldRefs(tm.qfmt, old, clean), afmt: renameFieldRefs(tm.afmt, old, clean) })),
    });
  };
  const move = (i: number, dir: -1 | 1) => {
    const j = i + dir;
    if (j < 0 || j >= draft.fields.length) return;
    const sortName = draft.fields[draft.sortField]?.name;
    const fields = moveItem(draft.fields, i, j);
    setFields(fields, { sortField: Math.max(0, fields.findIndex((f) => f.name === sortName)) });
    if (open === i) setOpen(j);
    onAnnounce(t('notetypes.fieldMoved', { name: draft.fields[i]!.name, pos: j + 1 }));
    requestAnimationFrame(() => document.querySelector<HTMLElement>(`[data-field-row="${j}"] [data-move="${dir}"]`)?.focus());
  };
  const add = () => {
    const name = uniqueName(newName.trim() || t('notetypes.newField'), names);
    const ord = Math.max(-1, ...draft.fields.map((f) => f.ord)) + 1;
    setFields([...draft.fields, { ord, name, config: { ...defaultConfig } }]);
    setNewName('');
    onAnnounce(t('notetypes.fieldAdded', { name }));
  };
  const remove = (i: number) => {
    const name = draft.fields[i]!.name;
    const fields = draft.fields.filter((_, k) => k !== i);
    setFields(fields, { sortField: Math.min(draft.sortField > i ? draft.sortField - 1 : draft.sortField === i ? 0 : draft.sortField, fields.length - 1) });
    setRemoving(null); setOpen(null);
    onAnnounce(t('notetypes.fieldRemoved', { name }));
  };

  return (
    <div className={s.section}>
      <ol className={s.fieldList}>
        {draft.fields.map((f, i) => {
          const dup = names.filter((n) => n.trim().toLowerCase() === f.name.trim().toLowerCase()).length > 1;
          return (
            <li key={`${f.ord}`} className={cx(s.fieldRow, open === i && s.fieldOpen)} data-field-row={i}>
              <div className={s.fieldMain}>
                <span className={s.fieldNum} aria-hidden>{i + 1}</span>
                <input
                  className={cx(s.fieldName, (dup || !f.name.trim()) && s.invalid)}
                  value={f.name}
                  aria-label={t('notetypes.fieldName', { n: i + 1 })}
                  aria-invalid={dup || !f.name.trim() || undefined}
                  onChange={(e) => rename(i, e.target.value)}
                  onKeyDown={(e) => { if (e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) { e.preventDefault(); move(i, e.key === 'ArrowUp' ? -1 : 1); } }}
                  dir="auto"
                />
                <span className={s.badges}>
                  {draft.sortField === i && <span className={s.badge}>{t('notetypes.sortsBy')}</span>}
                  {f.config.sticky && <Icon name="pin" size={14} label={t('editor.sticky')} />}
                  {f.config.rtl && <Icon name="rtl" size={14} label={t('editor.rtlToggle')} />}
                </span>
                <IconButton size="sm" icon="arrow-up" label={t('notetypes.moveUp')} data-move={-1} disabled={i === 0} onClick={() => move(i, -1)} />
                <IconButton size="sm" icon="arrow-down" label={t('notetypes.moveDown')} data-move={1} disabled={i === draft.fields.length - 1} onClick={() => move(i, 1)} />
                <IconButton size="sm" icon="settings" label={t('notetypes.fieldOptions', { name: f.name })} aria-expanded={open === i} onClick={() => setOpen(open === i ? null : i)} />
              </div>
              {dup && <p className={s.rowError} role="alert">{t('notetypes.duplicateField')}</p>}
              {open === i && (
                <div className={s.fieldOptions}>
                  <FormField label={t('editor.fields.description')} help={t('notetypes.descriptionHelp')}>
                    <Input value={f.config.description} onChange={(e) => update(i, { description: e.target.value })} />
                  </FormField>
                  <FormField label={t('notetypes.stickyLabel')} help={t('notetypes.stickyHelp')} inline>
                    <Switch checked={f.config.sticky} onChange={(v) => update(i, { sticky: v })} />
                  </FormField>
                  <FormField label={t('editor.fields.dirRtl')} help={t('notetypes.rtlHelp')} inline>
                    <Switch checked={f.config.rtl} onChange={(v) => update(i, { rtl: v })} />
                  </FormField>
                  <FormField label={t('editor.fields.plainText')} help={t('notetypes.plainHelp')} inline>
                    <Switch checked={f.config.plainText} onChange={(v) => update(i, { plainText: v })} />
                  </FormField>
                  <div className={s.optionActions}>
                    <Button size="sm" variant="ghost" icon="sort-asc" disabled={draft.sortField === i} onClick={() => onChange({ ...draft, sortField: i })}>{t('notetypes.sortByThis')}</Button>
                    <Button size="sm" variant="quiet-danger" icon="trash" disabled={draft.fields.length <= 1} onClick={() => setRemoving(i)}>{t('editor.fields.remove')}</Button>
                  </div>
                </div>
              )}
            </li>
          );
        })}
      </ol>
      <form className={s.addRow} onSubmit={(e) => { e.preventDefault(); add(); }}>
        <Input value={newName} onChange={(e) => setNewName(e.target.value)} placeholder={t('notetypes.newFieldPlaceholder')} aria-label={t('notetypes.newFieldPlaceholder')} />
        <Button type="submit" icon="plus">{t('editor.fields.add')}</Button>
      </form>
      <Dialog open={removing !== null} onClose={() => setRemoving(null)} destructive title={t('notetypes.removeFieldTitle', { name: removing !== null ? draft.fields[removing]?.name ?? '' : '' })}
        description={t('notetypes.removeFieldBody')} confirmLabel={t('editor.fields.remove')} onConfirm={() => { if (removing !== null) remove(removing); }} />
    </div>
  );
}
