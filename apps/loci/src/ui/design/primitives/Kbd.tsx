import type { ReactNode } from 'react';
import { cx } from './layer';
import { useTouchOnly } from './pointer';
import s from './Kbd.module.css';

export interface KbdProps {
  children: ReactNode;
  className?: string;
  /** show even on touch devices / when hints are turned off */
  always?: boolean;
}

const NAMES: Record<string, string> = { ctrl: 'Ctrl', shift: 'Shift', alt: 'Alt', meta: '⌘', enter: 'Enter', space: 'Space', esc: 'Esc', escape: 'Esc', delete: 'Delete', backspace: 'Backspace' };

/** Keycap. A string like "ctrl+z" renders as two caps. Hidden on touch devices unless `always`. */
export function Kbd({ children, className, always }: KbdProps) {
  const touch = useTouchOnly();
  // Touch-only devices never get keycaps (not even hidden ones: they would still be read out).
  if (touch && !always) return null;
  const hint = always ? undefined : 'kbd-hint';
  if (typeof children === 'string' && children.length > 1 && children.includes('+')) {
    const parts = children.split('+');
    return (
      <span className={cx(s.combo, hint, className)}>
        {parts.map((p, i) => <kbd key={i} className={s.kbd}>{NAMES[p.toLowerCase()] ?? p.toUpperCase()}</kbd>)}
      </span>
    );
  }
  const label = typeof children === 'string' ? (NAMES[children.toLowerCase()] ?? (children.length === 1 ? children.toUpperCase() : children)) : children;
  return <kbd className={cx(s.kbd, hint, className)}>{label}</kbd>;
}
