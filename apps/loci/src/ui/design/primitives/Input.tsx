import type { InputHTMLAttributes, Ref, TextareaHTMLAttributes } from 'react';
import { Icon, type IconName } from '../icons';
import { useFieldControl } from './Field';
import { cx } from './layer';
import s from './Field.module.css';

/** No `title`: explain a field with Field help or a Tooltip (internals §15). */
export interface InputProps extends Omit<InputHTMLAttributes<HTMLInputElement>, 'title'> {
  icon?: IconName;
  /** unit text shown inside the end of the field, e.g. "min" */
  suffix?: string;
  mono?: boolean;
  ref?: Ref<HTMLInputElement>;
}

export function Input({ icon, suffix, mono, className, id, ...rest }: InputProps) {
  const a11y = useFieldControl(id);
  const input = <input {...a11y} {...rest} className={cx(s.control, mono && s.mono, !icon && !suffix && className)} />;
  if (!icon && !suffix) return input;
  return (
    <div className={cx(s.affix, icon && s['with-icon'], suffix && s['with-suffix'], className)}>
      {icon && <Icon name={icon} size={18} />}
      {input}
      {suffix && <span className={s.suffix} aria-hidden>{suffix}</span>}
    </div>
  );
}

export interface TextareaProps extends TextareaHTMLAttributes<HTMLTextAreaElement> { mono?: boolean; ref?: Ref<HTMLTextAreaElement> }
export function Textarea({ mono, className, id, ...rest }: TextareaProps) {
  const a11y = useFieldControl(id);
  return <textarea {...a11y} {...rest} className={cx(s.control, mono && s.mono, className)} />;
}
