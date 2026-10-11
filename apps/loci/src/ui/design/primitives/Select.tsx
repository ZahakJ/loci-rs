import type { Ref, SelectHTMLAttributes } from 'react';
import { Icon } from '../icons';
import { useFieldControl } from './Field';
import { cx } from './layer';
import s from './Field.module.css';

export interface SelectOption<V extends string | number = string> { value: V; label: string; disabled?: boolean }
export interface SelectProps<V extends string | number = string> extends Omit<SelectHTMLAttributes<HTMLSelectElement>, 'onChange' | 'value'> {
  options: Array<SelectOption<V>>;
  value: V;
  onChange: (value: V) => void;
  ref?: Ref<HTMLSelectElement>;
}

/** Native select (best on every platform), styled. */
export function Select<V extends string | number = string>({ options, value, onChange, className, id, ...rest }: SelectProps<V>) {
  const a11y = useFieldControl(id);
  const numeric = typeof value === 'number';
  return (
    <div className={cx(s.select, className)}>
      <select
        {...a11y}
        {...rest}
        className={s.control}
        value={String(value)}
        onChange={(e) => onChange((numeric ? Number(e.target.value) : e.target.value) as V)}
      >
        {options.map((o) => <option key={String(o.value)} value={String(o.value)} disabled={o.disabled}>{o.label}</option>)}
      </select>
      <Icon name="chevron-down" size={16} />
    </div>
  );
}
