import { useId, useRef, type CSSProperties, type FormEvent, type ReactNode, type RefObject } from 'react';
import { createPortal } from 'react-dom';
import { Icon, type IconName } from '../icons';
import { Button } from './Button';
import { cx, useModalLayer, usePresence } from './layer';
import s from './Dialog.module.css';

export interface DialogProps {
  open: boolean;
  onClose: () => void;
  title: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
  /** when set, renders the confirm/cancel pair; the dialog is a <form> so Enter confirms */
  onConfirm?: () => void | Promise<void>;
  confirmLabel?: string;
  cancelLabel?: string;
  destructive?: boolean;
  busy?: boolean;
  confirmDisabled?: boolean;
  icon?: IconName;
  /** custom actions replace the confirm/cancel pair */
  actions?: ReactNode;
  width?: string;
  initialFocus?: RefObject<HTMLElement | null>;
  className?: string;
}

/** Modal dialog (confirm or small form). role=alertdialog when destructive. */
export function Dialog({
  open, onClose, title, description, children, onConfirm, confirmLabel = 'OK', cancelLabel = 'Cancel',
  destructive, busy, confirmDisabled, icon, actions, width, initialFocus, className,
}: DialogProps) {
  const { mounted, state } = usePresence(open, '--d-fast');
  const panel = useRef<HTMLFormElement>(null);
  const titleId = useId();
  const descId = useId();
  useModalLayer(panel, open && mounted, () => { if (!busy) onClose(); }, initialFocus);
  if (!mounted) return null;
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (onConfirm && !busy && !confirmDisabled) void onConfirm();
  };
  return createPortal(
    <div className={cx(s.root, destructive && s.destructive)} data-state={state}>
      <div className={s.scrim} onClick={() => !busy && onClose()} aria-hidden />
      <form
        ref={panel}
        role={destructive ? 'alertdialog' : 'dialog'}
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={description ? descId : undefined}
        tabIndex={-1}
        className={cx(s.panel, className)}
        style={width ? ({ '--dialog-w': width } as CSSProperties) : undefined}
        onSubmit={submit}
        noValidate
      >
        <div className={s.content}>
          {(icon || destructive) && <div className={s.icon} aria-hidden><Icon name={icon ?? 'warning'} /></div>}
          <h2 id={titleId} className={s.title}>{title}</h2>
          {description && <div id={descId} className={s.description}>{description}</div>}
          {children && <div className={s.body}>{children}</div>}
        </div>
        <div className={s.actions}>
          {actions ?? (
            <>
              <Button variant="ghost" onClick={onClose} disabled={busy} data-autofocus={destructive ? true : undefined}>{cancelLabel}</Button>
              {onConfirm && (
                <Button type="submit" variant={destructive ? 'danger' : 'primary'} loading={busy} disabled={confirmDisabled}>{confirmLabel}</Button>
              )}
            </>
          )}
        </div>
      </form>
    </div>,
    document.body,
  );
}
