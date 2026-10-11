/**
 * One rich-text field: a plain `contenteditable` (arbitrary Anki HTML survives untouched),
 * an HTML-source mode (mono textarea), paste cleaning, media paste/drop, cloze shortcuts,
 * Anki-style Enter (= <br>), per-field direction, sticky pin and description placeholder.
 * Formatting commands go through `document.execCommand` so the browser's undo stack keeps working.
 */
import { useCallback, useEffect, useId, useImperativeHandle, useLayoutEffect, useRef, useState, type ClipboardEvent, type DragEvent, type KeyboardEvent, type ReactNode, type Ref } from 'react';
import { t } from '@core/i18n/i18n';
import { IconButton } from '@ui/design/primitives/Button';
import { cx } from '@ui/design/primitives/layer';
import { toast } from '@ui/design/primitives/Toast';
import { errorMessage } from '@ui/api';
import { SEL_END, SEL_START, applyClozeAtMarkers, stripMarkers } from './cloze';
import { highlightClozes, releaseHighlights } from './highlight';
import { editorHtml, hydrateMedia, mediaMarkup, serializeEditor, setEditorHtml, storeMediaFile } from './media';
import { cleanPastedHtml, plainTextToHtml } from './paste';
import s from './FieldEditor.module.css';

export type FieldCommand =
  | 'bold' | 'italic' | 'underline' | 'subscript' | 'superscript' | 'list' | 'orderedList'
  | 'mathInline' | 'mathDisplay' | 'cloze' | 'clozeSame' | 'clear' | 'source';

export interface FieldEditorHandle {
  focus: (where?: 'start' | 'end') => void;
  run: (cmd: FieldCommand) => void;
  insertHtml: (html: string) => void;
  insertText: (text: string) => void;
  createLink: (url: string, text?: string) => void;
  insertFiles: (files: File[]) => Promise<void>;
  selectedText: () => string;
  isSource: () => boolean;
  root: () => HTMLElement | null;
}

export interface FieldEditorProps {
  label: string;
  value: string;
  onChange: (html: string) => void;
  description?: string;
  dir?: 'auto' | 'rtl' | 'ltr';
  onToggleDir?: () => void;
  sticky?: boolean;
  onToggleSticky?: () => void;
  /** edit as plain text: the HTML source view only */
  plainText?: boolean;
  /** text of the note's other fields, for picking cloze numbers across fields */
  clozeContext?: () => string;
  onActivate?: (handle: FieldEditorHandle) => void;
  onBlur?: () => void;
  /** Tab / Shift+Tab: return true when focus moved to another field */
  onTab?: (direction: 1 | -1) => boolean;
  onSubmit?: () => void;
  onSave?: () => void;
  /** a link or image request from a keyboard shortcut (ctrl+k) */
  onRequestLink?: () => void;
  onSourceChange?: (source: boolean) => void;
  error?: string;
  /** extra hint under the field (e.g. duplicate warning) */
  footer?: ReactNode;
  autoFocus?: boolean;
  compact?: boolean;
  ref?: Ref<FieldEditorHandle>;
}

const MEDIA_TYPES = /^(image|audio|video)\//;

export function FieldEditor({
  label, value, onChange, description, dir = 'auto', onToggleDir, sticky, onToggleSticky, plainText,
  clozeContext, onActivate, onBlur, onTab, onSubmit, onSave, onRequestLink, onSourceChange, error, footer, autoFocus, compact, ref,
}: FieldEditorProps) {
  const id = useId();
  const labelId = `${id}-label`;
  const root = useRef<HTMLDivElement>(null);
  const source = useRef<HTMLTextAreaElement>(null);
  const lastEmitted = useRef<string | null>(null);
  const savedRange = useRef<Range | null>(null);
  const plainPaste = useRef(false);
  const [sourceMode, setSourceMode] = useState(!!plainText);
  const [sourceText, setSourceText] = useState(value);
  const [dragging, setDragging] = useState(false);
  const [busy, setBusy] = useState(false);
  const isSource = sourceMode || !!plainText;

  const emit = useCallback((html: string) => {
    lastEmitted.current = html;
    onChange(html);
  }, [onChange]);

  const sync = useCallback(() => {
    const el = root.current;
    if (!el) return;
    if (el.innerHTML === '<br>') el.innerHTML = '';
    emit(serializeEditor(el));
    highlightClozes(el);
  }, [emit]);

  // External value → DOM (note loaded, fields cleared after Add, note type switched).
  useLayoutEffect(() => {
    if (value === lastEmitted.current) return;
    lastEmitted.current = value;
    setSourceText(value);
    const el = root.current;
    if (el) {
      setEditorHtml(el, value);
      highlightClozes(el);
    }
  }, [value]);

  useEffect(() => {
    const el = root.current;
    return () => { if (el) releaseHighlights(el); };
  }, []);

  useEffect(() => { if (plainText) setSourceMode(true); }, [plainText]);

  // ── selection helpers ──
  const inside = (n: Node | null) => !!n && !!root.current && root.current.contains(n);
  const saveSelection = () => {
    const sel = window.getSelection();
    if (sel && sel.rangeCount && inside(sel.anchorNode)) savedRange.current = sel.getRangeAt(0).cloneRange();
  };
  const restoreSelection = () => {
    const el = root.current;
    if (!el) return;
    if (document.activeElement !== el) el.focus({ preventScroll: true });
    const sel = window.getSelection();
    if (!sel) return;
    if (sel.rangeCount && inside(sel.anchorNode)) return;
    sel.removeAllRanges();
    if (savedRange.current && inside(savedRange.current.startContainer)) sel.addRange(savedRange.current);
    else { const r = document.createRange(); r.selectNodeContents(el); r.collapse(false); sel.addRange(r); }
  };

  const exec = (command: string, arg?: string) => {
    restoreSelection();
    document.execCommand(command, false, arg);
    sync();
  };

  const insertHtml = (html: string) => {
    if (isSource) { insertIntoSource(html); return; }
    restoreSelection();
    document.execCommand('insertHTML', false, editorHtml(html));
    if (root.current) hydrateMedia(root.current);
    sync();
  };
  const insertText = (text: string) => {
    if (isSource) { insertIntoSource(text); return; }
    restoreSelection();
    document.execCommand('insertText', false, text);
    sync();
  };
  const insertIntoSource = (text: string) => {
    const ta = source.current;
    if (!ta) return;
    ta.focus();
    ta.setRangeText(text, ta.selectionStart, ta.selectionEnd, 'end');
    setSourceText(ta.value);
    emit(ta.value);
  };

  const wrapMath = (display: boolean) => {
    const [open, close] = display ? ['\\[', '\\]'] : ['\\(', '\\)'];
    if (isSource) {
      const ta = source.current;
      if (!ta) return;
      const a = ta.selectionStart, b = ta.selectionEnd;
      ta.setRangeText(open + ta.value.slice(a, b) + close, a, b, 'select');
      ta.setSelectionRange(a + open.length, b + open.length);
      setSourceText(ta.value); emit(ta.value);
      return;
    }
    restoreSelection();
    const sel = window.getSelection();
    const text = sel?.toString() ?? '';
    document.execCommand('insertText', false, open + text + close);
    if (!text && sel) { sel.modify('move', 'backward', 'character'); sel.modify('move', 'backward', 'character'); }
    sync();
  };

  const cloze = (mode: 'next' | 'same') => {
    if (isSource) {
      const ta = source.current;
      if (!ta) return;
      const a = ta.selectionStart, b = ta.selectionEnd;
      const marked = ta.value.slice(0, a) + SEL_START + ta.value.slice(a, b) + SEL_END + ta.value.slice(b);
      const res = applyClozeAtMarkers(marked, mode, (clozeContext?.() ?? '') + ta.value);
      if (!res) return;
      const clean = stripMarkers(res.html);
      const s0 = res.html.indexOf(SEL_START), s1 = res.html.indexOf(SEL_END) - 1;
      ta.value = clean; ta.setSelectionRange(s0, s1); ta.focus();
      setSourceText(clean); emit(clean);
      return;
    }
    const el = root.current;
    if (!el) return;
    restoreSelection();
    const sel = window.getSelection();
    if (!sel || !sel.rangeCount) return;
    const range = sel.getRangeAt(0);
    const endR = range.cloneRange(); endR.collapse(false); endR.insertNode(document.createTextNode(SEL_END));
    const startR = range.cloneRange(); startR.collapse(true); startR.insertNode(document.createTextNode(SEL_START));
    const marked = serializeEditor(el, true);
    const res = applyClozeAtMarkers(marked, mode, (clozeContext?.() ?? '') + stripMarkers(marked));
    if (!res) { setEditorHtml(el, stripMarkers(el.innerHTML)); return; }
    setEditorHtml(el, res.html);
    restoreMarkers(el);
    sync();
  };

  const toggleSource = () => {
    if (plainText) return;
    if (!sourceMode) {
      setSourceText(root.current ? serializeEditor(root.current) : value);
      setSourceMode(true);
      onSourceChange?.(true);
      requestAnimationFrame(() => source.current?.focus());
    } else {
      setSourceMode(false);
      onSourceChange?.(false);
      requestAnimationFrame(() => {
        const el = root.current;
        if (!el) return;
        setEditorHtml(el, sourceText);
        highlightClozes(el);
        focusAt(el, 'end');
      });
    }
  };

  const insertFiles = async (files: File[]) => {
    const media = files.filter((f) => MEDIA_TYPES.test(f.type));
    if (!media.length) return;
    setBusy(true);
    try {
      for (const f of media) {
        const name = await storeMediaFile(f);
        const m = mediaMarkup(name, f.type);
        if (m.html) insertHtml(m.html); else if (m.text) insertText(m.text);
      }
    } catch (e) {
      toast({ message: t('editor.mediaFailed', { message: errorMessage(e) }), tone: 'danger' });
    } finally { setBusy(false); }
  };

  /** Pasted `data:` images become collection files, like Anki. */
  const internalizeDataImages = async () => {
    const el = root.current;
    if (!el) return;
    const imgs = Array.from(el.querySelectorAll<HTMLImageElement>('img[src^="data:"]'));
    if (!imgs.length) return;
    for (const img of imgs) {
      try {
        const blob = await (await fetch(img.src)).blob();
        const file = new File([blob], '', { type: blob.type });
        const name = await storeMediaFile(file);
        img.dataset.lociSrc = name;
      } catch { /* keep the data URI */ }
    }
    sync();
  };

  const handle: FieldEditorHandle = {
    focus: (where = 'end') => {
      if (isSource) { source.current?.focus(); return; }
      if (root.current) focusAt(root.current, where);
    },
    run: (cmd) => {
      switch (cmd) {
        case 'bold': case 'italic': case 'underline': case 'subscript': case 'superscript':
          if (isSource) return;
          exec(cmd); break;
        case 'list': if (!isSource) exec('insertUnorderedList'); break;
        case 'orderedList': if (!isSource) exec('insertOrderedList'); break;
        case 'clear': if (!isSource) { exec('removeFormat'); exec('unlink'); } break;
        case 'mathInline': wrapMath(false); break;
        case 'mathDisplay': wrapMath(true); break;
        case 'cloze': cloze('next'); break;
        case 'clozeSame': cloze('same'); break;
        case 'source': toggleSource(); break;
      }
    },
    insertHtml, insertText,
    createLink: (url, text) => {
      if (isSource) { insertIntoSource(`<a href="${url.replace(/"/g, '&quot;')}">${text || url}</a>`); return; }
      restoreSelection();
      const sel = window.getSelection();
      if (sel && sel.isCollapsed) document.execCommand('insertHTML', false, `<a href="${url.replace(/"/g, '&quot;')}">${(text || url).replace(/</g, '&lt;')}</a>`);
      else document.execCommand('createLink', false, url);
      sync();
    },
    insertFiles,
    selectedText: () => {
      if (isSource) { const ta = source.current; return ta ? ta.value.slice(ta.selectionStart, ta.selectionEnd) : ''; }
      const sel = window.getSelection();
      return sel && inside(sel.anchorNode) ? sel.toString() : savedRange.current?.toString() ?? '';
    },
    isSource: () => isSource,
    root: () => (isSource ? source.current : root.current),
  };
  const handleRef = useRef(handle);
  handleRef.current = handle;
  const stableHandle = useRef<FieldEditorHandle | null>(null);
  if (!stableHandle.current) {
    stableHandle.current = {
      focus: (w) => handleRef.current.focus(w),
      run: (c) => handleRef.current.run(c),
      insertHtml: (h) => handleRef.current.insertHtml(h),
      insertText: (x) => handleRef.current.insertText(x),
      createLink: (u, x) => handleRef.current.createLink(u, x),
      insertFiles: (f) => handleRef.current.insertFiles(f),
      selectedText: () => handleRef.current.selectedText(),
      isSource: () => handleRef.current.isSource(),
      root: () => handleRef.current.root(),
    };
  }
  useImperativeHandle(ref, () => stableHandle.current!, []);

  useEffect(() => {
    if (!autoFocus) return;
    const id = requestAnimationFrame(() => handleRef.current.focus('end'));
    return () => cancelAnimationFrame(id);
  }, [autoFocus]);

  // ── keys shared by both modes ──
  const onKeyCommon = (e: KeyboardEvent<HTMLElement>): boolean => {
    const mod = e.ctrlKey || e.metaKey;
    // The field owns Ctrl+Enter / Ctrl+S: stop the event here so a host's own handler (the Add
    // sheet listens on its form too) does not submit a second time.
    if (mod && e.key === 'Enter') { e.preventDefault(); e.stopPropagation(); if (e.repeat) return true; sync(); onSubmit?.(); return true; }
    if (mod && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 's') { e.preventDefault(); e.stopPropagation(); if (e.repeat) return true; sync(); onSave?.(); return true; }
    if (mod && e.shiftKey && e.code === 'KeyC') { e.preventDefault(); cloze(e.altKey ? 'same' : 'next'); return true; }
    if (mod && e.shiftKey && e.code === 'KeyX') { e.preventDefault(); toggleSource(); return true; }
    if (mod && e.code === 'KeyM') { e.preventDefault(); wrapMath(e.shiftKey); return true; }
    if (mod && !e.shiftKey && e.code === 'KeyK') { e.preventDefault(); saveSelection(); onRequestLink?.(); return true; }
    if (e.key === 'Tab' && !mod && !e.altKey && onTab) {
      if (onTab(e.shiftKey ? -1 : 1)) { e.preventDefault(); return true; }
    }
    return false;
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (onKeyCommon(e)) return;
    const mod = e.ctrlKey || e.metaKey;
    if (mod && e.shiftKey && e.code === 'KeyV') { plainPaste.current = true; return; }
    if (mod && e.code === 'Equal') { e.preventDefault(); exec(e.shiftKey ? 'superscript' : 'subscript'); return; }
    if (e.key === 'Enter' && !e.shiftKey && !mod && !e.nativeEvent.isComposing) {
      const sel = window.getSelection();
      const inList = sel?.anchorNode && (sel.anchorNode.nodeType === 1 ? (sel.anchorNode as Element) : sel.anchorNode.parentElement)?.closest('li');
      if (!inList) { e.preventDefault(); document.execCommand('insertLineBreak'); sync(); }
    }
  };

  const onPaste = (e: ClipboardEvent<HTMLDivElement>) => {
    const dt = e.clipboardData;
    const files = Array.from(dt.files).filter((f) => MEDIA_TYPES.test(f.type));
    const html = dt.getData('text/html');
    const text = dt.getData('text/plain');
    e.preventDefault();
    if (files.length && !html) { void insertFiles(files); plainPaste.current = false; return; }
    if (plainPaste.current || !html) insertHtml(plainTextToHtml(text));
    else { insertHtml(cleanPastedHtml(html)); void internalizeDataImages(); }
    plainPaste.current = false;
  };

  const onDrop = (e: DragEvent<HTMLDivElement>) => {
    setDragging(false);
    const files = Array.from(e.dataTransfer.files).filter((f) => MEDIA_TYPES.test(f.type));
    if (!files.length) return;
    e.preventDefault();
    const pos = (document as Document & { caretRangeFromPoint?: (x: number, y: number) => Range | null }).caretRangeFromPoint?.(e.clientX, e.clientY);
    root.current?.focus();
    if (pos) { const sel = window.getSelection(); sel?.removeAllRanges(); sel?.addRange(pos); }
    void insertFiles(files);
  };

  const errorId = error ? `${id}-error` : undefined;
  const descId = description ? `${id}-desc` : undefined;

  return (
    <div className={cx(s.field, compact && s.compact, isSource && s.sourceMode)} data-dragging={dragging || undefined}>
      <div className={s.head}>
        <span id={labelId} className={s.label}>{label}</span>
        <div className={s.tools}>
          {onToggleDir && (
            <IconButton size="sm" icon="rtl" label={t('editor.rtlToggle')} aria-pressed={dir === 'rtl'} onClick={onToggleDir} className={s.tool} />
          )}
          {!plainText && (
            <IconButton size="sm" icon="code" label={t('editor.htmlSource')} aria-pressed={sourceMode} onClick={toggleSource} className={s.tool} />
          )}
          {onToggleSticky && (
            <IconButton size="sm" icon="pin" label={t('editor.sticky')} aria-pressed={!!sticky} onClick={onToggleSticky} className={cx(s.tool, sticky && s.pinned)} />
          )}
        </div>
      </div>
      {isSource ? (
        <textarea
          ref={source}
          className={s.source}
          value={sourceText}
          spellCheck={false}
          aria-labelledby={labelId}
          aria-describedby={[descId, errorId].filter(Boolean).join(' ') || undefined}
          aria-invalid={error ? true : undefined}
          placeholder={description}
          dir={dir === 'auto' ? undefined : dir}
          rows={Math.min(10, Math.max(3, sourceText.split('\n').length + 1))}
          onChange={(e) => { setSourceText(e.target.value); emit(e.target.value); }}
          onKeyDown={(e) => { onKeyCommon(e); }}
          onFocus={() => onActivate?.(stableHandle.current!)}
          onBlur={() => onBlur?.()}
        />
      ) : null}
      <div
        ref={root}
        className={s.editable}
        hidden={isSource}
        contentEditable
        suppressContentEditableWarning
        role="textbox"
        aria-multiline="true"
        aria-labelledby={labelId}
        aria-describedby={[descId, errorId].filter(Boolean).join(' ') || undefined}
        aria-invalid={error ? true : undefined}
        aria-busy={busy || undefined}
        dir={dir}
        data-placeholder={description ?? ''}
        spellCheck
        onInput={sync}
        onKeyDown={onKeyDown}
        onKeyUp={saveSelection}
        onMouseUp={saveSelection}
        onPaste={onPaste}
        onDragOver={(e) => { if (Array.from(e.dataTransfer.items).some((i) => i.kind === 'file')) { e.preventDefault(); setDragging(true); } }}
        onDragLeave={() => setDragging(false)}
        onDrop={onDrop}
        onFocus={() => onActivate?.(stableHandle.current!)}
        onBlur={() => { saveSelection(); onBlur?.(); }}
      />
      {description && <span id={descId} className="sr-only">{description}</span>}
      {error && <p id={errorId} className={s.error} role="alert">{error}</p>}
      {footer}
    </div>
  );
}

function focusAt(el: HTMLElement, where: 'start' | 'end') {
  el.focus({ preventScroll: false });
  const sel = window.getSelection();
  if (!sel) return;
  const r = document.createRange();
  r.selectNodeContents(el);
  r.collapse(where === 'start');
  sel.removeAllRanges();
  sel.addRange(r);
}

/** Finds the selection sentinels, removes them and selects what lay between. */
function restoreMarkers(root: HTMLElement) {
  const find = (ch: string): [Text, number] | null => {
    const w = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    for (let n = w.nextNode() as Text | null; n; n = w.nextNode() as Text | null) {
      const i = n.data.indexOf(ch);
      if (i >= 0) { n.deleteData(i, 1); return [n, i]; }
    }
    return null;
  };
  const a = find(SEL_START);
  const b = find(SEL_END);
  root.focus({ preventScroll: true });
  const sel = window.getSelection();
  if (!sel || !a) return;
  const r = document.createRange();
  r.setStart(a[0], Math.min(a[1], a[0].length));
  if (b) r.setEnd(b[0], Math.min(b[1], b[0].length)); else r.collapse(true);
  sel.removeAllRanges();
  sel.addRange(r);
}
