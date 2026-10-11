/**
 * Note types (DESIGN §10): list with note counts; editor for fields and card templates with a
 * live preview; add (from an existing type), duplicate, rename and delete with confirmation.
 * Saving warns when the change regenerates or removes cards.
 */
import { useEffect, useMemo, useState } from 'react';
import { t } from '@core/i18n/i18n';
import { NotetypeKind, type Notetype } from '@core/types';
import { api, errorMessage, useQuery } from '@ui/api';
import { Icon } from '@ui/design/icons';
import { Button, IconButton } from '@ui/design/primitives/Button';
import { Dialog } from '@ui/design/primitives/Dialog';
import { EmptyState } from '@ui/design/primitives/EmptyState';
import { Field } from '@ui/design/primitives/Field';
import { Input } from '@ui/design/primitives/Input';
import { Menu } from '@ui/design/primitives/Menu';
import { Select } from '@ui/design/primitives/Select';
import { Skeleton } from '@ui/design/primitives/Skeleton';
import { Tabs } from '@ui/design/primitives/Tabs';
import { cx, useMedia } from '@ui/design/primitives/layer';
import { toast } from '@ui/design/primitives/Toast';
import { formatCount } from '@ui/hooks/format';
import { Page } from '@ui/shell/Page';
import { isOcclusionType } from '../edit/NoteForm';
import { FieldsEditor } from './FieldsEditor';
import { TemplatesEditor } from './TemplatesEditor';
import { uniqueName } from './templateText';
import s from './Notetypes.module.css';

type Section = 'fields' | 'templates';

/** What saving a draft changes structurally (drives the regeneration warning). */
export function structuralChanges(saved: Notetype, draft: Notetype): { removedFields: string[]; removedTemplates: string[]; addedTemplates: string[]; templatesChanged: boolean } {
  const fieldOrds = new Set(draft.fields.map((f) => f.ord));
  const tplOrds = new Set(draft.templates.map((x) => x.ord));
  const savedTpl = new Set(saved.templates.map((x) => x.ord));
  return {
    removedFields: saved.fields.filter((f) => !fieldOrds.has(f.ord)).map((f) => f.name),
    removedTemplates: saved.templates.filter((x) => !tplOrds.has(x.ord)).map((x) => x.name),
    addedTemplates: draft.templates.filter((x) => !savedTpl.has(x.ord)).map((x) => x.name),
    templatesChanged: JSON.stringify(saved.templates.map((x) => [x.qfmt, x.afmt])) !== JSON.stringify(draft.templates.map((x) => [x.qfmt, x.afmt])),
  };
}

export default function NotetypesView() {
  const list = useQuery((a) => a.listNotetypes(), [], ['notetypes']);
  const counts = useQuery(async (a) => {
    const nts = await a.listNotetypes();
    const entries = await Promise.all(nts.map(async (n) => [n.id, (await a.searchIds(`note:"${n.name.replace(/"/g, '\\"')}"`, 'notes')).length] as const));
    return new Map(entries);
  }, [], ['notes', 'notetypes']);
  const phone = useMedia('(max-width: 899px)');
  const [selected, setSelected] = useState<number | null>(null);
  const [draft, setDraft] = useState<Notetype | null>(null);
  const [section, setSection] = useState<Section>('fields');
  const [dialog, setDialog] = useState<'add' | 'rename' | 'delete' | 'save' | 'discard' | null>(null);
  const [pendingPick, setPendingPick] = useState<number | null>(null);
  const [name, setName] = useState('');
  const [base, setBase] = useState<number>(0);
  const [busy, setBusy] = useState(false);
  const [announce, setAnnounce] = useState('');
  const [showList, setShowList] = useState(true);
  const [savedVersion, setSavedVersion] = useState(0);

  const types = list.data ?? [];
  const saved = types.find((n) => n.id === selected) ?? null;
  useEffect(() => { if (selected === null && types[0] && !phone) setSelected(types[0].id); }, [types, selected, phone]);
  useEffect(() => { setDraft(saved ? structuredClone(saved) : null); }, [saved]);
  const dirty = !!draft && !!saved && JSON.stringify(draft) !== JSON.stringify(saved);
  const changes = useMemo(() => (saved && draft ? structuralChanges(saved, draft) : null), [saved, draft]);
  const invalid = !!draft && (draft.fields.some((f) => !f.name.trim()) || new Set(draft.fields.map((f) => f.name.trim().toLowerCase())).size !== draft.fields.length);

  const pick = (id: number, force = false) => {
    if (id === selected && !phone) return;
    if (dirty && !force) { setPendingPick(id); setDialog('discard'); return; }
    setSelected(id); setSection('fields');
    if (phone) setShowList(false);
  };

  const save = async () => {
    if (!draft) return;
    setBusy(true);
    try {
      await api.saveNotetype(draft);
      setSavedVersion((v) => v + 1);
      toast({ message: t('notetypes.saved', { name: draft.name }), icon: 'check', tone: 'success' });
      setDialog(null);
    } catch (e) { toast({ message: errorMessage(e), tone: 'danger' }); }
    finally { setBusy(false); }
  };
  const requestSave = () => {
    if (!changes) return;
    if (changes.removedFields.length || changes.removedTemplates.length || changes.addedTemplates.length || changes.templatesChanged) setDialog('save');
    else void save();
  };

  const create = async () => {
    const src = types.find((n) => n.id === base) ?? types[0];
    if (!src) return;
    setBusy(true);
    try {
      const { id: _id, mtime: _m, usn: _u, ...rest } = src;
      const { originalId: _o, ...config } = rest.config;
      // id 0 = a new note type (the API assigns one)
      const nt = await api.saveNotetype({ ...structuredClone(rest), id: 0, name: name.trim(), config });
      setSelected(nt.id); setDialog(null); setShowList(false);
      toast({ message: t('notetypes.created', { name: nt.name }), icon: 'check' });
    } catch (e) { toast({ message: errorMessage(e), tone: 'danger' }); }
    finally { setBusy(false); }
  };
  const duplicate = async (src: Notetype) => {
    try {
      const { id: _id, mtime: _m, usn: _u, ...rest } = src;
      const nt = await api.saveNotetype({ ...structuredClone(rest), id: 0, name: uniqueName(t('notetypes.copyOf', { name: src.name }), types.map((x) => x.name)) });
      setSelected(nt.id);
      toast({ message: t('notetypes.created', { name: nt.name }), icon: 'copy' });
    } catch (e) { toast({ message: errorMessage(e), tone: 'danger' }); }
  };
  const rename = async () => {
    if (!saved) return;
    setBusy(true);
    try { await api.saveNotetype({ ...(draft ?? saved), name: name.trim() }); setDialog(null); }
    catch (e) { toast({ message: errorMessage(e), tone: 'danger' }); }
    finally { setBusy(false); }
  };
  const remove = async () => {
    if (!saved) return;
    setBusy(true);
    try {
      await api.deleteNotetype(saved.id);
      toast({ message: t('notetypes.deleted', { name: saved.name }), icon: 'trash' });
      setSelected(null); setDialog(null); setShowList(true);
    } catch (e) { toast({ message: errorMessage(e), tone: 'danger' }); setDialog(null); }
    finally { setBusy(false); }
  };

  const kindIcon = (n: Notetype) => (isOcclusionType(n) ? 'occlusion' : n.kind === NotetypeKind.Cloze ? 'cloze' : 'cards');
  const nameTaken = (nm: string, except?: number) => types.some((x) => x.id !== except && x.name.toLowerCase() === nm.trim().toLowerCase());
  const noteCount = saved ? counts.data?.get(saved.id) ?? 0 : 0;

  const listEl = (
    <nav className={s.list} aria-label={t('notetypes.listLabel')}>
      <ul role="list" className={s.items}>
        {types.map((n) => {
          const c = counts.data?.get(n.id);
          return (
            <li key={n.id}>
              <button type="button" className={s.item} aria-current={n.id === selected ? 'true' : undefined} onClick={() => pick(n.id)}>
                <Icon name={kindIcon(n)} size={18} className={s.itemIcon} />
                <span className={s.itemText}>
                  <span className={s.itemName}>{n.name}</span>
                  <span className={s.itemMeta}>{t('notetypes.meta', { fields: n.fields.length, cards: n.templates.length })}</span>
                </span>
                <span className={cx(s.itemCount, 'num')}>{c === undefined ? '' : <><span aria-hidden>{formatCount(c)}</span><span className="sr-only">{t('notetypes.notesCount', { count: c })}</span></>}</span>
              </button>
            </li>
          );
        })}
      </ul>
      <Button variant="secondary" icon="plus" block onClick={() => { setName(''); setBase(types[0]?.id ?? 0); setDialog('add'); }}>{t('notetypes.add')}</Button>
    </nav>
  );

  const editorEl = draft && saved ? (
    <section className={s.editor} aria-labelledby="nt-title">
      <header className={s.editorHead}>
        {phone && <IconButton icon="back" label={t('common.back')} onClick={() => setShowList(true)} />}
        <div className={s.editorTitles}>
          <h2 id="nt-title" className={s.editorTitle}>{saved.name}</h2>
          <p className={s.editorMeta}>{t('notetypes.notesCount', { count: noteCount })} · {saved.kind === NotetypeKind.Cloze ? t('notetypes.kindCloze') : t('notetypes.kindStandard')}</p>
        </div>
        <Menu
          label={t('notetypes.actions')}
          items={[
            { id: 'rename', label: t('notetypes.rename'), icon: 'edit', onSelect: () => { setName(saved.name); setDialog('rename'); } },
            { id: 'dup', label: t('notetypes.duplicate'), icon: 'copy', onSelect: () => void duplicate(saved) },
            { separator: true, id: 's' },
            { id: 'delete', label: t('common.delete'), icon: 'trash', danger: true, onSelect: () => setDialog('delete') },
          ]}
          trigger={(p) => <IconButton {...p} icon="more" label={t('notetypes.actions')} />}
        />
      </header>
      <Tabs label={t('notetypes.sections')} value={section} onChange={setSection}
        items={[{ value: 'fields', label: t('editor.fields.title'), count: draft.fields.length }, { value: 'templates', label: t('editor.templates.title'), count: draft.templates.length }]} />
      <div className={s.editorBody}>
        {section === 'fields'
          ? <FieldsEditor draft={draft} onChange={setDraft} onAnnounce={setAnnounce} />
          : <TemplatesEditor draft={draft} saved={saved} dirty={dirty} onChange={setDraft} onAnnounce={setAnnounce}
              onSaveAndPreview={invalid ? undefined : requestSave} savedVersion={savedVersion} saving={busy} />}
      </div>
      <footer className={cx(s.saveBar, dirty && s.saveBarDirty)}>
        <p className={s.saveState} role="status">{dirty ? t('notetypes.unsaved') : t('notetypes.allSaved')}</p>
        <Button variant="ghost" disabled={!dirty || busy} onClick={() => setDraft(structuredClone(saved))}>{t('notetypes.revert')}</Button>
        <Button variant="primary" disabled={!dirty || invalid} loading={busy} onClick={requestSave}>{t('common.save')}</Button>
      </footer>
    </section>
  ) : null;

  return (
    <Page title={t('notetypes.title')} subtitle={t('notetypes.subtitle')} back={{ href: '#/settings', label: t('settings.title') }} width="wide">
      {list.loading ? (
        <div className={s.layout} aria-busy="true"><div className={s.list}>{[0, 1, 2, 3].map((i) => <Skeleton key={i} height="3.25rem" radius="md" />)}</div><Skeleton height="24rem" radius="lg" /></div>
      ) : types.length === 0 ? (
        <EmptyState icon="layers" title={t('notetypes.empty')} />
      ) : phone ? (
        showList || !editorEl ? listEl : editorEl
      ) : (
        <div className={s.layout}>{listEl}{editorEl}</div>
      )}
      <span className="sr-only" aria-live="polite">{announce}</span>

      <Dialog open={dialog === 'discard'} onClose={() => setDialog(null)} destructive title={t('notetypes.discardTitle')} description={t('notetypes.discard')}
        confirmLabel={t('notetypes.discardConfirm')} onConfirm={() => { setDialog(null); if (pendingPick !== null) pick(pendingPick, true); }} />
      <Dialog open={dialog === 'add'} onClose={() => setDialog(null)} title={t('notetypes.add')} confirmLabel={t('notetypes.create')} busy={busy}
        confirmDisabled={!name.trim() || nameTaken(name)} onConfirm={() => void create()}>
        <div className={s.dialogForm}>
          <Field label={t('notetypes.name')} error={name.trim() && nameTaken(name) ? t('notetypes.nameTaken') : undefined}><Input value={name} onChange={(e) => setName(e.target.value)} data-autofocus /></Field>
          <Field label={t('notetypes.basedOn')} help={t('notetypes.basedOnHelp')}>
            <Select value={base} onChange={setBase} options={types.map((n) => ({ value: n.id, label: n.name }))} />
          </Field>
        </div>
      </Dialog>
      <Dialog open={dialog === 'rename'} onClose={() => setDialog(null)} title={t('notetypes.renameTitle')} confirmLabel={t('notetypes.rename')} busy={busy}
        confirmDisabled={!name.trim() || nameTaken(name, saved?.id)} onConfirm={() => void rename()}>
        <Field label={t('notetypes.name')} error={name.trim() && nameTaken(name, saved?.id) ? t('notetypes.nameTaken') : undefined}><Input value={name} onChange={(e) => setName(e.target.value)} data-autofocus /></Field>
      </Dialog>
      <Dialog open={dialog === 'delete'} onClose={() => setDialog(null)} destructive busy={busy} title={t('notetypes.deleteTitle', { name: saved?.name ?? '' })}
        description={noteCount > 0 ? t('notetypes.deleteInUse', { count: noteCount }) : t('notetypes.deleteBody')}
        confirmLabel={t('common.delete')} confirmDisabled={noteCount > 0} onConfirm={() => void remove()} />
      <Dialog open={dialog === 'save'} onClose={() => setDialog(null)} title={t('notetypes.saveTitle')} confirmLabel={t('notetypes.saveAnyway')} busy={busy}
        destructive={!!changes && (changes.removedFields.length > 0 || changes.removedTemplates.length > 0)} onConfirm={() => void save()}
        description={t('notetypes.regenerate', { count: noteCount })}>
        {changes && (
          <ul className={s.changeList}>
            {changes.removedFields.map((f) => <li key={`f${f}`}>{t('notetypes.changeRemovedField', { name: f })}</li>)}
            {changes.removedTemplates.map((f) => <li key={`r${f}`}>{t('notetypes.changeRemovedTemplate', { name: f })}</li>)}
            {changes.addedTemplates.map((f) => <li key={`a${f}`}>{t('notetypes.changeAddedTemplate', { name: f })}</li>)}
            {changes.templatesChanged && !changes.addedTemplates.length && !changes.removedTemplates.length && <li>{t('notetypes.changeTemplates')}</li>}
          </ul>
        )}
      </Dialog>
    </Page>
  );
}
