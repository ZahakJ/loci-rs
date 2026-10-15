/**
 * The Add / Edit note form (internals §8), shared by the Add sheet and the full-page editor.
 * Render-prop: the host decides where `body` and `footer` go (Sheet body/footer, or a page).
 *
 * Add: note type and deck pickers (remembered), Ctrl+Enter adds and keeps focus in the first
 * field, sticky fields survive, toast with Undo. Edit: Ctrl+S / Save, toast with Undo.
 * Duplicate check on the first field (debounced); live preview; image-occlusion canvas for IO.
 */
import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import { t } from '@core/i18n/i18n';
import { stripHtml } from '@core/text';
import { clozeNumbersInString } from '@core/template/cloze';
import { parseOcclusionField, firstImageName } from '@core/template/occlusion';
import { NotetypeKind, type DeckTreeNode, type Note, type Notetype } from '@core/types';
import { api, errorMessage, useQuery } from '@ui/api';
import { Button } from '@ui/design/primitives/Button';
import { Kbd } from '@ui/design/primitives/Kbd';
import { useBottomBar } from '@ui/design/primitives/bottomBar';
import { useTouchOnly } from '@ui/design/primitives/pointer';
import { Skeleton } from '@ui/design/primitives/Skeleton';
import { cx } from '@ui/design/primitives/layer';
import { toast } from '@ui/design/primitives/Toast';
import { Icon } from '@ui/design/icons';
import { NoteEditor, type NoteEditorHandle } from '@ui/editor/NoteEditor';
import { CardPreview } from './CardPreview';
import { Combobox, deckOptions } from './Combobox';
import { defaultDeckId, readLastChoice, writeLastChoice } from './lastChoice';
import { OcclusionEditor } from './occlusion/OcclusionEditor';
import { singleFlight } from './singleFlight';
import { TagInput } from './TagInput';
import s from './NoteForm.module.css';

export interface NoteFormRender { body: ReactNode; footer: ReactNode; title: string; dirty: boolean }

export interface NoteFormProps {
  noteId?: number | null;
  deckId?: number | null;
  /** 'sheet': preview below the fields; 'page': preview beside them on wide screens */
  layout: 'sheet' | 'page';
  children: (r: NoteFormRender) => ReactNode;
  onSaved?: () => void;
}

export function isOcclusionType(nt: Notetype | undefined): boolean {
  if (!nt) return false;
  if (nt.config.builtin === 'image-occlusion' || nt.config.originalStockKind === 6) return true;
  const names = nt.fields.map((f) => f.name.toLowerCase());
  return nt.kind === NotetypeKind.Cloze && names.includes('occlusion') && names.includes('image');
}

function ioIndices(nt: Notetype): { occ: number; img: number } {
  const idx = (n: string, fallback: number) => { const i = nt.fields.findIndex((f) => f.name.toLowerCase() === n); return i >= 0 ? i : fallback; };
  return { occ: idx('occlusion', 0), img: idx('image', 1) };
}

/** Moves field contents to a new note type: same name first, then same position. */
export function remapFields(from: Notetype | undefined, to: Notetype, fields: string[]): string[] {
  if (!from) return to.fields.map(() => '');
  const used = new Set<number>();
  const out = to.fields.map((f) => {
    const i = from.fields.findIndex((g) => g.name === f.name);
    if (i >= 0 && fields[i]) { used.add(i); return fields[i]!; }
    return '';
  });
  to.fields.forEach((f, k) => {
    if (out[k]) return;
    if (!used.has(k) && fields[k] && k < from.fields.length) { out[k] = fields[k]!; used.add(k); }
  });
  return out;
}

function flattenDecks(node: DeckTreeNode | undefined): Array<{ id: number; name: string }> {
  if (!node) return [];
  const out: Array<{ id: number; name: string }> = [];
  const walk = (n: DeckTreeNode) => { for (const c of n.children) { out.push({ id: c.deck.id, name: c.deck.name }); walk(c); } };
  walk(node);
  return out;
}

export function NoteForm({ noteId, deckId: deckProp, layout, children, onSaved }: NoteFormProps) {
  const edit = noteId != null;
  const touch = useTouchOnly();
  // The Add sheet's save bar sits at the bottom of the sheet: toasts go above it (EditNoteView registers its own bar).
  const [footerEl, setFooterEl] = useState<HTMLDivElement | null>(null);
  const footerRef = useMemo(() => ({ current: footerEl }), [footerEl]);
  useBottomBar(footerRef, layout === 'sheet' && footerEl !== null);
  const notetypes = useQuery((a) => a.listNotetypes(), [], ['notetypes']);
  const tree = useQuery((a) => a.deckTree(), [], ['decks']);
  const decks = useMemo(() => flattenDecks(tree.data), [tree.data]);
  const last = useMemo(() => readLastChoice(), []);

  const [note, setNote] = useState<Note | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [ntid, setNtid] = useState<number | null>(null);
  const [deckId, setDeckId] = useState<number | null>(deckProp ?? null);
  const [fields, setFields] = useState<string[]>([]);
  const [tags, setTags] = useState<string[]>([]);
  const [sticky, setSticky] = useState<boolean[]>([]);
  const [dupes, setDupes] = useState<number[]>([]);
  const [errors, setErrors] = useState<Record<number, string | undefined>>({});
  const [formError, setFormError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [showPreview, setShowPreview] = useState<boolean>(() => readLastChoice().previewOpen ?? true);
  const [focusKey, setFocusKey] = useState(0);
  const editor = useRef<NoteEditorHandle>(null);
  const nt = notetypes.data?.find((n) => n.id === ntid);
  const io = isOcclusionType(nt);

  // ── initial note type / deck (add) or the note (edit) ──
  useEffect(() => {
    if (edit) {
      let live = true;
      api.getNote(noteId!).then((n) => {
        if (!live) return;
        setNote(n); setNtid(n.ntid); setFields(n.fields); setTags(n.tags); setDirty(false);
      }).catch((e) => live && setLoadError(errorMessage(e)));
      return () => { live = false; };
    }
    return undefined;
  }, [edit, noteId]);

  useEffect(() => {
    if (edit || ntid !== null || !notetypes.data?.length) return;
    const pick = notetypes.data.find((n) => n.id === last.notetypeId) ?? notetypes.data[0]!;
    setNtid(pick.id);
    setFields(pick.fields.map(() => ''));
    setSticky(pick.fields.map((f) => f.config.sticky));
  }, [edit, ntid, notetypes.data, last.notetypeId]);

  useEffect(() => {
    if (edit || deckId !== null || !decks.length) return;
    setDeckId(defaultDeckId(decks, last.deckId));
  }, [edit, deckId, decks, last.deckId]);

  useEffect(() => { if (deckProp != null) setDeckId(deckProp); }, [deckProp]);

  // ── duplicate check on the first field ──
  // (No state is set synchronously in this effect: it runs on every keystroke, and an update
  // scheduled from an effect each commit kept React's nested-update counter climbing while typing
  // fast — "Maximum update depth exceeded", E-1.)
  const first = fields[0] ?? '';
  const firstEmpty = !nt || io || !stripHtml(first).trim();
  useEffect(() => {
    if (firstEmpty || !nt) return;
    let live = true;
    const timer = window.setTimeout(() => {
      api.findDuplicates(nt.id, first)
        .then((ids) => { if (live) setDupes(ids.filter((id) => id !== noteId)); })
        .catch(() => { if (live) setDupes([]); });
    }, 400);
    return () => { live = false; window.clearTimeout(timer); };
  }, [nt, firstEmpty, first, noteId]);
  const shownDupes = firstEmpty ? [] : dupes;

  const changeNotetype = (id: number) => {
    const to = notetypes.data?.find((n) => n.id === id);
    if (!to || id === ntid) return;
    setFields((f) => remapFields(nt, to, f));
    setSticky(to.fields.map((f) => f.config.sticky));
    setNtid(id);
    setErrors({});
    setFormError(null);
    writeLastChoice({ notetypeId: id });
    setFocusKey((k) => k + 1);
  };
  const changeDeck = (id: number) => { setDeckId(id); writeLastChoice({ deckId: id }); };
  const createDeck = async (name: string) => {
    try { const d = await api.createDeck(name); changeDeck(d.id); } catch (e) { toast({ message: errorMessage(e), tone: 'danger' }); }
  };

  const setField = useCallback((i: number, html: string) => {
    setFields((prev) => { if (prev[i] === html) return prev; const next = [...prev]; next[i] = html; return next; });
    setDirty(true);
    setErrors((e) => (e[i] ? { ...e, [i]: undefined } : e));
    setFormError(null);
  }, []);

  const validate = (): boolean => {
    if (!nt) return false;
    if (io) {
      const { occ, img } = ioIndices(nt);
      if (!firstImageName(fields[img] ?? '')) { setFormError(t('editor.occlusion.needImage')); return false; }
      if (!parseOcclusionField(fields[occ] ?? '').some((sh) => sh.kind !== 'text')) { setFormError(t('editor.occlusion.noShapes')); return false; }
      return true;
    }
    const f0 = fields[0] ?? '';
    if (!stripHtml(f0).trim() && !/<img|\[sound:/i.test(f0)) {
      setErrors({ 0: t('editor.emptyFirstField') });
      editor.current?.focusField(0);
      return false;
    }
    if (nt.kind === NotetypeKind.Cloze && clozeNumbersInString(fields.join('\n')).length === 0) {
      setFormError(t(touch ? 'editor.noClozesTouch' : 'editor.noClozes'));
      editor.current?.focusField(0);
      return false;
    }
    return true;
  };

  const undoToast = (message: string) => toast({
    message, icon: 'check', tone: 'success',
    action: { label: t('common.undo'), onClick: () => { void api.undo().then((r) => r && toast({ message: t('undo.done', { label: r.label }), icon: 'undo' })); } },
  });

  const submitNow = async () => {
    if (!nt) return;
    if (edit) { await saveNow(); return; }
    if (deckId === null || !validate()) return;
    setBusy(true);
    try {
      const res = await api.addNote(nt.id, deckId, fields, tags);
      undoToast(t('editor.added', { count: res.cardIds.length }));
      writeLastChoice({ notetypeId: nt.id, deckId });
      setFields((f) => f.map((v, i) => (sticky[i] ? v : '')));
      setDupes([]);
      setDirty(false);
      setFocusKey((k) => k + 1);
      const skip = io ? Object.values(ioIndices(nt)) : [];
      const firstVisible = nt.fields.findIndex((_, i) => !skip.includes(i));
      requestAnimationFrame(() => editor.current?.focusField(Math.max(0, firstVisible), 'start'));
    } catch (e) {
      setFormError(errorMessage(e));
    } finally { setBusy(false); }
  };

  const saveNow = async () => {
    if (!edit || !note || !validate()) return;
    setBusy(true);
    try {
      const res = await api.updateNote(note.id, fields, tags);
      setDirty(false);
      const extra = res.cardsAdded || res.cardsRemoved ? ` ${t('editor.cardsChanged', { added: res.cardsAdded, removed: res.cardsRemoved })}` : '';
      undoToast(t('editor.saved') + extra);
      onSaved?.();
    } catch (e) {
      setFormError(errorMessage(e));
    } finally { setBusy(false); }
  };

  // One guard for every way of submitting (Ctrl+Enter in a field or the tags, Ctrl+S, the button):
  // a second request while the first is still in flight is ignored, never queued.
  const latest = useRef({ submitNow, saveNow });
  latest.current = { submitNow, saveNow };
  const flight = useMemo(() => singleFlight((kind: 'submit' | 'save') => (kind === 'save' ? latest.current.saveNow() : latest.current.submitNow())), []);
  const submit = () => flight('submit');
  const save = () => flight('save');

  const onKeyDown = (e: KeyboardEvent) => {
    const mod = e.ctrlKey || e.metaKey;
    if (mod && e.key === 'Enter') { e.preventDefault(); if (!e.repeat) void submit(); }
    else if (mod && !e.shiftKey && e.key.toLowerCase() === 's') { e.preventDefault(); void (edit ? save() : submit()); }
  };

  const title = edit ? t('editor.editTitle') : t('editor.addTitle');
  const ready = !!nt && (edit ? !!note : deckId !== null || decks.length === 0);

  const ntOptions = (notetypes.data ?? []).map((n) => ({ value: n.id, label: n.name, icon: (isOcclusionType(n) ? 'occlusion' : n.kind === NotetypeKind.Cloze ? 'cloze' : 'cards') as 'occlusion' | 'cloze' | 'cards' }));
  const ioIdx = nt && io ? ioIndices(nt) : null;
  const togglePreview = () => setShowPreview((v) => { writeLastChoice({ previewOpen: !v }); return !v; });
  const previewToggle = (
    <Button size="sm" variant="ghost" icon="eye" aria-pressed={showPreview} onClick={togglePreview}>
      {t('editor.preview')}
    </Button>
  );

  let body: ReactNode;
  if (loadError) {
    body = <p className={s.loadError} role="alert">{loadError}</p>;
  } else if (!ready || !nt) {
    body = (
      <div className={s.loading} aria-busy="true" aria-label={t('common.loading')}>
        <Skeleton height="2.5rem" radius="md" /><Skeleton height="5rem" radius="md" /><Skeleton height="5rem" radius="md" />
      </div>
    );
  } else {
    const disclosure = (
      <button type="button" className={s.disclosure} aria-expanded={showPreview} onClick={togglePreview}>
        <Icon name="chevron-down" size={16} />
        {t('editor.preview')}
      </button>
    );
    const previewEl = showPreview ? (
      <CardPreview
        notetype={nt}
        fields={fields}
        title={t('editor.preview')}
        headerStart={layout === 'sheet' ? disclosure : undefined}
        className={cx(s.preview, layout === 'sheet' && s.previewSheet)}
      />
    ) : null;
    const dupeFooter = shownDupes.length > 0 ? (
      <p className={s.dupe} role="status">
        <Icon name="warning" size={16} />
        <span>{t('editor.duplicate')}</span>
        <a href={`#/browse?q=${encodeURIComponent(`nid:${shownDupes.join(',')}`)}`} className={s.dupeLink}>{t('editor.showDuplicates')}</a>
      </p>
    ) : null;
    body = (
      <div className={cx(s.form, layout === 'page' && s.page, layout === 'page' && showPreview && s.withPreview)} onKeyDown={onKeyDown}>
        <div className={s.main}>
          <div className={cx(s.pickers, edit && s.single)}>
            <Combobox
              label={t('editor.noteType')}
              value={ntid}
              options={ntOptions}
              onChange={changeNotetype}
              icon={io ? 'occlusion' : nt.kind === NotetypeKind.Cloze ? 'cloze' : 'cards'}
              disabled={edit}
            />
            {!edit && (
              <Combobox
                label={t('editor.deck')}
                value={deckId}
                options={deckOptions(decks)}
                onChange={changeDeck}
                onCreate={(name) => void createDeck(name)}
                createLabel={(name) => t('editor.createDeck', { name })}
                icon="deck"
                placeholder={t('editor.chooseDeck')}
              />
            )}
          </div>
          {io && ioIdx && (
            <OcclusionEditor
              key={`${nt.id}:${focusKey}:${noteId ?? ''}`}
              occlusion={fields[ioIdx.occ] ?? ''}
              image={fields[ioIdx.img] ?? ''}
              defaultMode={nt.config.occlusionMode}
              onChange={({ occlusion, image }) => { setField(ioIdx.occ, occlusion); setField(ioIdx.img, image); }}
            />
          )}
          <NoteEditor
            key={nt.id}
            ref={editor}
            notetype={nt}
            fields={fields}
            onFieldChange={setField}
            sticky={edit ? undefined : sticky}
            onToggleSticky={edit ? undefined : (i) => setSticky((st) => st.map((v, k) => (k === i ? !v : v)))}
            onSubmit={() => void submit()}
            onSave={() => void (edit ? save() : submit())}
            hidden={ioIdx ? [ioIdx.occ, ioIdx.img] : undefined}
            errors={errors}
            footers={dupeFooter ? { 0: dupeFooter } : undefined}
            autoFocus={!io}
            toolbarEnd={layout === 'page' ? previewToggle : undefined}
          />
          <TagInput label={t('editor.tags')} tags={tags} onChange={(v) => { setTags(v); setDirty(true); }} placeholder={t('editor.tagsPlaceholder')} onSubmit={() => void submit()} />
          {formError && <p className={s.formError} role="alert"><Icon name="warning" size={16} />{formError}</p>}
          {/* Sheet (phones too): the preview folds below the fields, so the toolbar keeps to formatting. */}
          {layout === 'sheet' && (showPreview ? previewEl : <div className={s.previewHead}>{disclosure}</div>)}
        </div>
        {layout === 'page' && previewEl && <aside className={s.side}>{previewEl}</aside>}
      </div>
    );
  }

  const footer = (
    <div ref={setFooterEl} className={s.footer}>
      {/* Keyboard hint: keycaps through <Kbd>, and no words about keys on touch-only devices. */}
      {touch ? <span className={s.hint} /> : (
        <span className={cx(s.hint, 'kbd-hint')} aria-hidden>
          {edit ? <><Kbd>ctrl+s</Kbd> {t('editor.toSave')}</> : <><Kbd>ctrl+enter</Kbd> {t('editor.toAdd')}</>}
        </span>
      )}
      {edit ? (
        <Button variant="primary" onClick={() => void save()} loading={busy} disabled={!ready || !dirty || busy}>{dirty ? t('common.save') : t('common.saved')}</Button>
      ) : (
        <Button variant="primary" icon="plus" onClick={() => void submit()} loading={busy} disabled={!ready || busy}>{t('editor.addButton')}</Button>
      )}
    </div>
  );

  return <>{children({ body, footer, title, dirty })}</>;
}
