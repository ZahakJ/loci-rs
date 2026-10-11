import type { AnchorHTMLAttributes, ButtonHTMLAttributes, ReactNode, Ref } from 'react';
import { Icon, type IconName } from '../icons';
import { Kbd } from './Kbd';
import { Tooltip } from './Tooltip';
import { cx } from './layer';
import s from './Button.module.css';

export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger' | 'quiet-danger';
export type ButtonSize = 'sm' | 'md' | 'lg';

interface CommonProps {
  variant?: ButtonVariant;
  size?: ButtonSize;
  /** keyboard hint shown as a keycap at the end (hidden on touch) */
  kbd?: string;
  icon?: IconName;
  iconEnd?: IconName;
  loading?: boolean;
  block?: boolean;
  className?: string;
  children?: ReactNode;
}

export interface ButtonProps extends CommonProps, Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children' | 'className' | 'title'> {
  ref?: Ref<HTMLButtonElement>;
}

function Inner({ icon, iconEnd, loading, kbd, children, size }: CommonProps) {
  const iconSize = size === 'sm' ? 16 : 18;
  return (
    <>
      {loading ? <span className={s.spinner} aria-hidden /> : icon ? <Icon name={icon} size={iconSize} /> : null}
      {children != null && <span className={s.label}>{children}</span>}
      {iconEnd && <Icon name={iconEnd} size={iconSize} />}
      {kbd && <Kbd className={s.kbd}>{kbd}</Kbd>}
    </>
  );
}

export function Button({ variant = 'secondary', size = 'md', kbd, icon, iconEnd, loading, block, className, children, type = 'button', disabled, ...rest }: ButtonProps) {
  return (
    <button
      type={type}
      className={cx(s.button, s[variant], s[size], block && s.block, className)}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      {...rest}
    >
      <Inner {...{ icon, iconEnd, loading, kbd, size }}>{children}</Inner>
    </button>
  );
}

export interface LinkButtonProps extends CommonProps, Omit<AnchorHTMLAttributes<HTMLAnchorElement>, 'children' | 'className' | 'title'> {
  href: string;
  ref?: Ref<HTMLAnchorElement>;
}

/** A link styled as a button (navigation). */
export function LinkButton({ variant = 'secondary', size = 'md', kbd, icon, iconEnd, loading, block, className, children, ...rest }: LinkButtonProps) {
  return (
    <a className={cx(s.button, s[variant], s[size], block && s.block, className)} {...rest}>
      <Inner {...{ icon, iconEnd, loading, kbd, size }}>{children}</Inner>
    </a>
  );
}

export interface IconButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children' | 'title'> {
  icon: IconName;
  /** accessible name (required); also the tooltip text */
  label: string;
  size?: ButtonSize;
  variant?: 'ghost' | 'secondary' | 'primary';
  iconSize?: number;
  /** tooltip text (default: the label, plus the shortcut when given); false for none */
  tooltip?: ReactNode | false;
  tooltipPlacement?: 'top' | 'bottom';
  ref?: Ref<HTMLButtonElement>;
}

/** Icon-only button. Always named (`label`) and, by default, explained by the Tooltip primitive, never by `title`. */
export function IconButton({ icon, label, size = 'md', variant = 'ghost', iconSize, className, type = 'button', tooltip, tooltipPlacement, ...rest }: IconButtonProps) {
  const button = (
    <button
      type={type}
      aria-label={label}
      className={cx(s['icon-button'], s[size], variant !== 'ghost' && s[variant], className)}
      {...rest}
    >
      <Icon name={icon} size={iconSize ?? (size === 'sm' ? 18 : 20)} />
    </button>
  );
  if (tooltip === false) return button;
  return <Tooltip content={tooltip ?? label} placement={tooltipPlacement}>{button}</Tooltip>;
}
