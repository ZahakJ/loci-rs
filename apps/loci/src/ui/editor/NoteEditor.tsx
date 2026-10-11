/**
 * The fields of one note with a shared toolbar: Tab / Shift+Tab move between fields, Ctrl+Enter
 * submits, Ctrl+S saves, per-field direction toggle and sticky pins. Hosts the link dialog and
 * the hidden file pickers for images and audio.
 */
import { useImperativeHandle, useRef, useState, type ChangeEvent, type ReactNode, type Ref } from 'react';
import { t } from '@core/i18n/i18n';
import { NotetypeKind, type Notetype } from '@core/types';
import { Dialog } from '@ui/design/primitives/Dialog';
import { Field } from '@ui/design/primitives/Field';
import { Input } from '@ui/design/primitives/Input';
import { cx } from '@ui/design/primitives/layer';
import { EditorToolbar } from './EditorToolbar';
import { FieldEditor, type FieldEditorHandle } from './FieldEditor';
import s from './NoteEditor.module.css';

export interface NoteEditorHandle {
  focusField: (index: number, where?: 'start' | 'end') => void;
}

export interface NoteEditorProps {
  notetype: Notetype;
  fields: string[];
  onFieldChange: (index: number, html: string) => void;
  /** add mode: sticky pins per field */
  sticky?: boolean[];
  onToggleSticky?: (index: number) => void;
  onSubmit?: () => void;
  onSave?: () => void;
  onFieldBlur?: (index: number) => void;
  /** field indices not to render (image occlusion edits those with its own canvas) */
  hidden?: number[];
  errors?: Record<number, string | undefined>;
  footers?: Record<number, ReactNode>;
  autoFocus?: boolean;
  compact?: boolean;
  /** toolbar sticks to the top of the scrolling container */
  stickyToolbar?: boolean;
  toolbarEnd?: ReactNode;
  className?: string;
  ref?: Ref<NoteEditorHandle>;
}

export function NoteEditor({
  notetype, fields, onFieldChange, sticky, onToggleSticky, onSubmit, onSave, onFieldBlur, hidden, errors, footers,
  autoFocus, compact, stickyToolbar = true, toolbarEnd, className, ref,
}: NoteEditorProps) {
  const handles = useRef<Array<FieldEditorHandle | null>>([]);
  const [active, setActive] = useState<FieldEditorHandle | null>(null);
  const activeRef = useRef<FieldEditorHandle | null>(null);
  const [dirs, setDirs] = useState<Record<number, 'rtl' | 'auto'>>({});
  const [linkOpen, setLinkOpen] = useState(false);
  const [linkUrl, setLinkUrl] = useState('https://');
  const [linkText, setLinkText] = useState('');
  const [hasSelection, setHasSelection] = useState(false);
  const imageInput = useRef<HTMLInputElement>(null);
  const audioInput = useRef<HTMLInputElement>(null);
  const fieldsRef = useRef(fields);
  fieldsRef.current = fields;

  const visible = notetype.fields.map((f, i) => ({ f, i })).filter(({ i }) => !hidden?.includes(i));
  const isCloze = notetype.kind === NotetypeKind.Cloze && notetype.config.builtin !== 'image-occlusion';

  useImperativeHandle(ref, () => ({
    focusField: (index, where = 'end') => handles.current[index]?.focus(where),
  }), []);

  const move = (from: number, dir: 1 | -1): boolean => {
    const pos = visible.findIndex((v) => v.i === from);
    const next = visible[pos + dir];
    if (!next) return false;
    handles.current[next.i]?.focus(dir === 1 ? 'end' : 'end');
    return true;
  };

  const activate = (h: FieldEditorHandle) => { activeRef.current = h; setActive(h); };

  const openLink = () => {
    const sel = activeRef.current?.selectedText() ?? '';
    setHasSelection(!!sel);
    setLinkText(sel);
    setLinkUrl('https://');
    setLinkOpen(true);
  };

  const onPick = (e: ChangeEvent<HTMLInputElement>) => {
    const files = Array.from(e.target.files ?? []);
    e.target.value = '';
    if (files.length) void activeRef.current?.insertFiles(files);
  };

  return (
    <div className={cx(s.editor, className)}>
      <EditorToolbar
        className={cx(s.toolbar, stickyToolbar && s.stuck)}
        active={active}
        cloze={isCloze}
        onLink={openLink}
        onImage={() => imageInput.current?.click()}
        onAudio={() => audioInput.current?.click()}
        end={toolbarEnd}
      />
      <div className={cx(s.fields, compact && s.compact)}>
        {visible.map(({ f, i }) => (
          <FieldEditor
            key={`${notetype.id}:${f.ord}`}
            ref={(h) => { handles.current[i] = h; }}
            label={f.name}
            value={fields[i] ?? ''}
            onChange={(html) => onFieldChange(i, html)}
            description={f.config.description || undefined}
            dir={dirs[i] ?? (f.config.rtl ? 'rtl' : 'auto')}
            onToggleDir={() => setDirs((d) => ({ ...d, [i]: (d[i] ?? (f.config.rtl ? 'rtl' : 'auto')) === 'rtl' ? 'auto' : 'rtl' }))}
            sticky={sticky?.[i]}
            onToggleSticky={onToggleSticky ? () => onToggleSticky(i) : undefined}
            plainText={f.config.plainText}
            clozeContext={isCloze ? () => fieldsRef.current.filter((_, k) => k !== i).join('\n') : undefined}
            onActivate={activate}
            onBlur={() => onFieldBlur?.(i)}
            onTab={(dir) => move(i, dir)}
            onSubmit={onSubmit}
            onSave={onSave}
            onRequestLink={openLink}
            error={errors?.[i]}
            footer={footers?.[i]}
            autoFocus={autoFocus && i === visible[0]?.i}
            compact={compact}
          />
        ))}
      </div>
      <input ref={imageInput} type="file" accept="image/*" multiple hidden onChange={onPick} />
      <input ref={audioInput} type="file" accept="audio/*,video/*" multiple hidden onChange={onPick} />
      <Dialog
        open={linkOpen}
        onClose={() => { setLinkOpen(false); activeRef.current?.focus(); }}
        title={t('editor.link')}
        confirmLabel={t('editor.insertLink')}
        confirmDisabled={!/^(https?:\/\/.+|mailto:.+|#.+)/i.test(linkUrl.trim())}
        onConfirm={() => {
          setLinkOpen(false);
          const url = linkUrl.trim();
          requestAnimationFrame(() => activeRef.current?.createLink(url, hasSelection ? undefined : linkText.trim() || url));
        }}
      >
        <div className={s.dialogFields}>
          <Field label={t('editor.linkUrl')}>
            <Input type="url" value={linkUrl} onChange={(e) => setLinkUrl(e.target.value)} data-autofocus autoComplete="off" spellCheck={false} />
          </Field>
          {!hasSelection && (
            <Field label={t('editor.linkText')} optional>
              <Input value={linkText} onChange={(e) => setLinkText(e.target.value)} />
            </Field>
          )}
        </div>
      </Dialog>
    </div>
  );
}
