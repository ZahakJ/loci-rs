import type { Ref } from 'react';
import { useFieldControl } from './Field';
import { cx } from './layer';
import s from './Switch.module.css';

export interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  /** accessible label when not inside a Field */
  label?: string;
  disabled?: boolean;
  id?: string;
  className?: string;
  ref?: Ref<HTMLButtonElement>;
}

export function Switch({ checked, onChange, label, disabled, id, className, ref }: SwitchProps) {
  const a11y = useFieldControl(id);
  return (
    <button
      ref={ref}
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      {...a11y}
      className={cx(s.switch, className)}
      onClick={() => onChange(!checked)}
    >
      <span className={s.thumb} aria-hidden />
    </button>
  );
}
