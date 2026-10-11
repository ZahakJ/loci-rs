/**
 * Editor pane for the current Browse row: the note's fields (field editor engine), tags and a
 * preview of the current card. Saves on blur and shortly after typing stops; a pending save is
 * flushed when the row changes or the pane unmounts.
 */
import { useCallback, useEffect, useRef, useState } from 'react';
import { t } from '@core/i18n/i18n';
import type { Note, Notetype } from '@core/types';
import { api, errorMessage, useQuery } from '@ui/api';
import { Icon } from '@ui/design/icons';
import { IconButton, LinkButton } from '@ui/design/primitives/Button';
import { Skeleton } from '@ui/design/primitives/Skeleton';
import { cx } from '@ui/design/primitives/layer';
import { NoteEditor } from '@ui/editor/NoteEditor';
import { CardPreview } from '../edit/CardPreview';
import { isOcclusionType } from '../edit/NoteForm';
import { TagInput } from '../edit/TagInput';
import { cardMetaParts } from './cardMeta';
import s from './RowEditor.module.css';

export interface RowEditorProps {
  noteId: number;
  cardId: number;
  deckName?: string;
  onBack?: () => void;
  /** opens card info for this row's card */
  onInfo?: () => void;
  className?: string;
}

type Status = 'idle' | 'dirty' | 'saving' | 'saved' | 'error';

export function RowEditor({ noteId, cardId, deckName, onBack, onInfo, className }: RowEditorProps) {
  const [note, setNote] = useState<Note | null>(null);
  const [nt, setNt] = useState<Notetype | null>(null);
  const [ord, setOrd] = useState(0);
  const [fields, setFields] = useState<string[]>([]);
  const [tags, setTags] = useState<string[]>([]);
  const [status, setStatus] = useState<Status>('idle');
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState(true);
  const pending = useRef<{ id: number; fields: string[]; tags: string[] } | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const facts = useQuery(async (a) => {
    const [card, info] = await Promise.all([a.getCard(cardId), a.explainCard(cardId).catch(() => null)]);
    return { card, info };
  }, [cardId], ['cards', 'decks']);
  const meta = facts.data && facts.data.card.id === cardId ? cardMetaParts(facts.data.card, facts.data.info, deckName) : deckName ? [deckName.split('::').join(' / ')] : [];

  const flush = useCallback(async () => {
    window.clearTimeout(timer.current);
    const p = pending.current;
    if (!p) return;
    pending.current = null;
    setStatus('saving');
    try {
      await api.updateNote(p.id, p.fields, p.tags);
      setStatus((st) => (st === 'saving' ? 'saved' : st));
    } catch (e) { setStatus('error'); setError(errorMessage(e)); }
  }, []);

  useEffect(() => {
    let live = true;
    setNote(null); setStatus('idle'); setError(null);
    Promise.all([api.getNote(noteId), api.getCard(cardId).catch(() => null)]).then(async ([n, c]) => {
      const type = await api.getNotetype(n.ntid);
      if (!live) return;
      setNote(n); setNt(type); setFields(n.fields); setTags(n.tags); setOrd(c?.ord ?? 0);
    }).catch((e) => live && setError(errorMessage(e)));
    return () => { live = false; void flush(); };
  }, [noteId, cardId, flush]);

  const schedule = (f: string[], tg: string[]) => {
    if (!note) return;
    pending.current = { id: note.id, fields: f, tags: tg };
    setStatus('dirty');
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => void flush(), 900);
  };

  const fieldsRef = useRef(fields);
  fieldsRef.current = fields;
  const onField = (i: number, html: string) => {
    const prev = fieldsRef.current;
    if (prev[i] === html) return;
    const next = [...prev]; next[i] = html;
    fieldsRef.current = next;
    setFields(next);
    schedule(next, tags);
  };

  const io = isOcclusionType(nt ?? undefined);
  const statusText = status === 'saving' ? t('browse.editor.saving') : status === 'saved' ? t('browse.editor.saved') : status === 'dirty' ? t('browse.editor.unsaved') : status === 'error' ? t('browse.editor.failed') : '';

  return (
    <section className={cx(s.pane, className)} aria-label={t('browse.editor.label')}>
      <header className={s.head}>
        {onBack && <IconButton icon="back" label={t('common.back')} onClick={() => { void flush(); onBack(); }} />}
        <div className={s.titles}>
          <p className={s.kind}>{nt?.name ?? ' '}</p>
          {meta.length > 0 && (
            <p className={s.deck} title={meta.join(' · ')} aria-label={`${t('browse.meta.label')}: ${meta.join(', ')}`}>
              <Icon name="deck" size={13} />
              <span className={s.metaText}>{meta.join(' · ')}</span>
            </p>
          )}
        </div>
        <span className={cx(s.status, status === 'error' && s.statusError)} role="status" aria-live="polite">{statusText}</span>
        {onInfo && <IconButton size="sm" variant="ghost" icon="info" label={t('browse.cardInfo')} title={`${t('browse.cardInfo')} (I)`} onClick={onInfo} className={s.full} />}
        <LinkButton size="sm" variant="ghost" icon="external" href={`#/note/${noteId}`} onClick={() => void flush()} aria-label={t('browse.editor.openFull')} title={t('browse.editor.openFull')} className={s.full} />
      </header>
      <div className={s.scroll}>
        {error && <p className={s.error} role="alert">{error}</p>}
        {!note || !nt ? (
          <div className={s.loading} aria-busy="true"><Skeleton height="1rem" width="40%" /><Skeleton height="3rem" radius="md" /><Skeleton height="1rem" width="30%" /><Skeleton height="3rem" radius="md" /></div>
        ) : (
          <div className={s.form} onBlur={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node)) void flush(); }}>
            {io && (
              <p className={s.ioNote}>
                <Icon name="occlusion" size={16} />
                <span>{t('browse.editor.ioHint')}</span>
                <a href={`#/note/${noteId}`}>{t('browse.editor.openFull')}</a>
              </p>
            )}
            <NoteEditor
              key={note.id}
              notetype={nt}
              fields={fields}
              onFieldChange={onField}
              onFieldBlur={() => void flush()}
              onSave={() => void flush()}
              onSubmit={() => void flush()}
              hidden={io ? nt.fields.map((f, i) => (['occlusion', 'image'].includes(f.name.toLowerCase()) ? i : -1)).filter((i) => i >= 0) : undefined}
              compact
              stickyToolbar
            />
            <TagInput label={t('editor.tags')} tags={tags} onChange={(v) => { setTags(v); schedule(fields, v); }} placeholder={t('editor.tagsPlaceholder')} />
            <div className={s.previewHead}>
              <button type="button" className={s.disclosure} aria-expanded={preview} onClick={() => setPreview((v) => !v)}>
                <Icon name="chevron-down" size={16} />{t('editor.preview')}
              </button>
            </div>
            {preview && <CardPreview notetype={nt} fields={fields} ord={ord} />}
          </div>
        )}
      </div>
    </section>
  );
}
