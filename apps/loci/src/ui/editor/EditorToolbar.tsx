/**
 * Formatting toolbar for the active field. Buttons never take focus from the field with the
 * pointer (mousedown is prevented); from the keyboard the field restores its selection first.
 * Pressed states follow the selection (bold, italic, …).
 */
import { useEffect, useRef, useState, type MouseEvent, type ReactNode } from 'react';
import { t } from '@core/i18n/i18n';
import { Icon, type IconName } from '@ui/design/icons';
import { Menu } from '@ui/design/primitives/Menu';
import { cx } from '@ui/design/primitives/layer';
import { useTouchOnly } from '@ui/design/primitives/pointer';
import type { FieldCommand, FieldEditorHandle } from './FieldEditor';
import s from './EditorToolbar.module.css';
import { Tooltip } from '../design/primitives/Tooltip';

export interface EditorToolbarProps {
  active: FieldEditorHandle | null;
  /** show the cloze button (cloze note types) */
  cloze?: boolean;
  onLink: () => void;
  onImage: () => void;
  onAudio: () => void;
  className?: string;
  /** extra controls at the end (e.g. preview toggle) */
  end?: ReactNode;
}

const STATE_COMMANDS = ['bold', 'italic', 'underline', 'subscript', 'superscript', 'insertUnorderedList'] as const;
type StateCmd = typeof STATE_COMMANDS[number];

export function EditorToolbar({ active, cloze, onLink, onImage, onAudio, className, end }: EditorToolbarProps) {
  const [pressed, setPressed] = useState<Partial<Record<StateCmd, boolean>>>({});
  const [source, setSource] = useState(false);
  const coarse = useTouchOnly();
  // Last values pushed to state: selectionchange fires on every keystroke, and setting state that
  // did not change would re-render the toolbar each time (and feed React's nested-update count).
  const seen = useRef({ pressed: '', source: false });

  useEffect(() => {
    const update = () => {
      const root = active?.root();
      const src = !!active?.isSource();
      if (src !== seen.current.source) { seen.current.source = src; setSource(src); }
      const sel = window.getSelection();
      if (!root || !sel || !sel.anchorNode || !root.contains(sel.anchorNode)) return;
      const next: Partial<Record<StateCmd, boolean>> = {};
      for (const c of STATE_COMMANDS) { try { next[c] = document.queryCommandState(c); } catch { next[c] = false; } }
      const sig = STATE_COMMANDS.map((c) => (next[c] ? 1 : 0)).join('');
      if (sig !== seen.current.pressed) { seen.current.pressed = sig; setPressed(next); }
    };
    update();
    document.addEventListener('selectionchange', update);
    return () => document.removeEventListener('selectionchange', update);
  }, [active]);

  const disabled = !active;
  const run = (cmd: FieldCommand) => { active?.run(cmd); if (cmd === 'source') setSource((v) => { seen.current.source = !v; return !v; }); };
  const keep = (e: MouseEvent) => e.preventDefault();

  const ctx: BtnCtx = { pressed, source, disabled, run };

  return (
    <div role="toolbar" aria-label={t('editor.toolbar')} className={cx(s.bar, className)}>
      {cloze && (
        <>
          {/* Cloze notes: the one tool that matters comes first, never scrolled out of view. */}
          <Tooltip content={coarse ? t('editor.clozeTouchHint') : `${t('editor.clozeNew')} (Ctrl+Shift+C) · ${t('editor.clozeSame')} (Ctrl+Alt+Shift+C)`}>
            <button
              type="button"
              className={cx(s.btn, s.cloze)}
              disabled={disabled}
              onMouseDown={keep}
              onClick={(e) => run(e.altKey ? 'clozeSame' : 'cloze')}
            >
              <Icon name="cloze" size={18} />
              <span>{t('editor.cloze')}</span>
            </button>
          </Tooltip>
          <span className={s.sep} aria-hidden />
        </>
      )}
      <div className={s.group}>
        <Btn ctx={ctx} icon="bold" label={t('editor.bold')} cmd="bold" kbd="Ctrl+B" state="bold" textOnly />
        <Btn ctx={ctx} icon="italic" label={t('editor.italic')} cmd="italic" kbd="Ctrl+I" state="italic" textOnly />
        <Btn ctx={ctx} icon="underline" label={t('editor.underline')} cmd="underline" kbd="Ctrl+U" state="underline" textOnly />
      </div>
      <span className={s.sep} aria-hidden />
      <div className={s.group}>
        <Btn ctx={ctx} icon="subscript" label={t('editor.subscript')} cmd="subscript" kbd="Ctrl+=" state="subscript" textOnly />
        <Btn ctx={ctx} icon="superscript" label={t('editor.superscript')} cmd="superscript" kbd="Ctrl+Shift+=" state="superscript" textOnly />
      </div>
      <span className={s.sep} aria-hidden />
      <div className={s.group}>
        <Btn ctx={ctx} icon="list" label={t('editor.list')} cmd="list" state="insertUnorderedList" textOnly />
        <Btn ctx={ctx} icon="link" label={t('editor.link')} kbd="Ctrl+K" onClick={onLink} />
        <Btn ctx={ctx} icon="image" label={t('editor.image')} onClick={onImage} />
        <Btn ctx={ctx} icon="audio" label={t('editor.audio')} onClick={onAudio} />
        <Menu
          label={t('editor.maths')}
          align="start"
          items={[
            { id: 'inline', label: t('editor.mathsInline'), kbd: 'ctrl+m', onSelect: () => run('mathInline') },
            { id: 'display', label: t('editor.mathsBlock'), kbd: 'ctrl+shift+m', onSelect: () => run('mathDisplay') },
          ]}
          trigger={(p) => (
            <Tooltip content={t('editor.maths')}>
              <button type="button" {...p} className={s.btn} aria-label={t('editor.maths')} disabled={disabled} onMouseDown={keep}>
                <Icon name="maths" size={18} />
              </button>
            </Tooltip>
          )}
        />
      </div>
      <span className={s.sep} aria-hidden />
      <div className={s.group}>
        <Btn ctx={ctx} icon="eraser" label={t('editor.clearFormatting')} cmd="clear" textOnly />
        <Btn ctx={ctx} icon="code" label={t('editor.htmlSource')} cmd="source" kbd="Ctrl+Shift+X" />
      </div>
      {end && <div className={s.end}>{end}</div>}
    </div>
  );
}

interface BtnCtx { pressed: Partial<Record<StateCmd, boolean>>; source: boolean; disabled: boolean; run: (cmd: FieldCommand) => void }
interface BtnProps { ctx: BtnCtx; icon: IconName; label: string; cmd?: FieldCommand; kbd?: string; onClick?: () => void; state?: StateCmd; textOnly?: boolean }

function Btn({ ctx, icon, label, cmd, kbd, onClick, state, textOnly }: BtnProps) {
  return (
    <Tooltip content={kbd ? `${label} (${kbd})` : label}>
      <button
        type="button"
        className={s.btn}
        aria-label={label}
        aria-pressed={state ? !!ctx.pressed[state] && !ctx.source : cmd === 'source' ? ctx.source : undefined}
        disabled={ctx.disabled || (textOnly && ctx.source)}
        onMouseDown={(e: MouseEvent) => e.preventDefault()}
        onClick={() => (onClick ? onClick() : cmd && ctx.run(cmd))}
      >
        <Icon name={icon} size={18} />
      </button>
    </Tooltip>
  );
}
