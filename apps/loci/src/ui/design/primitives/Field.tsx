import { createContext, useContext, useId, type ReactNode } from 'react';
import { Icon } from '../icons';
import { cx } from './layer';
import s from './Field.module.css';

interface FieldCtx { id: string; describedBy?: string; invalid: boolean }
const Ctx = createContext<FieldCtx | null>(null);

/** Controls (Input, Select, Switch, Slider) pick up id / aria-describedby / aria-invalid from the nearest Field. */
export function useFieldControl(id?: string): { id: string; 'aria-describedby'?: string; 'aria-invalid'?: true } {
  const ctx = useContext(Ctx);
  const own = useId();
  return {
    id: id ?? ctx?.id ?? own,
    'aria-describedby': ctx?.describedBy,
    'aria-invalid': ctx?.invalid ? true : undefined,
  };
}

export interface FieldProps {
  label: ReactNode;
  help?: ReactNode;
  error?: ReactNode;
  optional?: boolean;
  /** label and control on one row (switches) */
  inline?: boolean;
  /** hide the label visually but keep it for screen readers */
  hideLabel?: boolean;
  children: ReactNode;
  className?: string;
  id?: string;
}

export function Field({ label, help, error, optional, inline, hideLabel, children, className, id }: FieldProps) {
  const own = useId();
  const controlId = id ?? own;
  const helpId = help ? `${controlId}-help` : undefined;
  const errorId = error ? `${controlId}-error` : undefined;
  const describedBy = [helpId, errorId].filter(Boolean).join(' ') || undefined;
  return (
    <Ctx.Provider value={{ id: controlId, describedBy, invalid: !!error }}>
      <div className={cx(s.field, inline && s.inline, className)}>
        <label htmlFor={controlId} className={cx(s.label, hideLabel && 'sr-only')}>
          {label}
          {optional && <span className={s.optional}>(optional)</span>}
        </label>
        {children}
        {help && <div id={helpId} className={s.help}>{help}</div>}
        {error && <div id={errorId} className={s.error} role="alert"><Icon name="warning" size={16} />{error}</div>}
      </div>
    </Ctx.Provider>
  );
}
