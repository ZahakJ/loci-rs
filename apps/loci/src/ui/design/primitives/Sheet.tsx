import { useId, useRef, type CSSProperties, type ReactNode, type RefObject } from 'react';
import { createPortal } from 'react-dom';
import { IconButton } from './Button';
import { cx, useModalLayer, usePresence } from './layer';
import s from './Sheet.module.css';

export interface SheetProps {
  open: boolean;
  onClose: () => void;
  title: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
  /** extra controls in the header, before the close button */
  headerActions?: ReactNode;
  /** desktop width (CSS length); default 36rem */
  width?: string;
  /** phone presentation: full screen (default) or bottom sheet */
  phone?: 'full' | 'bottom';
  initialFocus?: RefObject<HTMLElement | null>;
  closeLabel?: string;
  className?: string;
  bodyClassName?: string;
}

/** Side sheet (desktop) / full or bottom sheet (phone). Focus trap, Esc closes, focus returns, background inert. */
export function Sheet({ open, onClose, title, children, footer, headerActions, width, phone = 'full', initialFocus, closeLabel = 'Close', className, bodyClassName }: SheetProps) {
  const { mounted, state } = usePresence(open, '--d-base');
  const panel = useRef<HTMLDivElement>(null);
  const titleId = useId();
  useModalLayer(panel, open && mounted, onClose, initialFocus);
  if (!mounted) return null;
  return createPortal(
    <div className={cx(s.root, phone === 'bottom' && s.bottom)} data-state={state}>
      <div className={s.scrim} onClick={onClose} aria-hidden />
      <div
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        className={cx(s.panel, className)}
        style={width ? ({ '--sheet-w': width } as CSSProperties) : undefined}
      >
        <header className={s.header}>
          <h2 id={titleId} className={s.title}>{title}</h2>
          {headerActions}
          <IconButton icon="close" label={closeLabel} onClick={onClose} />
        </header>
        <div className={cx(s.body, bodyClassName)}>{children}</div>
        {footer && <footer className={s.footer}>{footer}</footer>}
      </div>
    </div>,
    document.body,
  );
}
