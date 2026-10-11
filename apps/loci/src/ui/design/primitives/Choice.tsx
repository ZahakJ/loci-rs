import { useEffect, useRef, type InputHTMLAttributes, type ReactNode, type Ref } from 'react';
import { cx } from './layer';
import s from './Choice.module.css';

interface ChoiceBase extends Omit<InputHTMLAttributes<HTMLInputElement>, 'type' | 'onChange' | 'title' | 'children'> {
  /** visible label; without one, pass `aria-label` */
  label?: ReactNode;
  /** a line of help under the label */
  description?: ReactNode;
  /** class for the row (the <label> that is the hit area) */
  className?: string;
  ref?: Ref<HTMLInputElement>;
}

function Row({ kind, label, description, className, mark, input }: { kind: 'radio' | 'checkbox'; label?: ReactNode; description?: ReactNode; className?: string; mark: ReactNode; input: ReactNode }) {
  const bare = label == null && description == null;
  return (
    <label className={cx(s.row, s[kind], bare && s.bare, className)}>
      {input}
      <span className={s.mark} aria-hidden>{mark}</span>
      {!bare && (
        <span className={s.text}>
          {label != null && <span className={s.label}>{label}</span>}
          {description != null && <span className={s.description}>{description}</span>}
        </span>
      )}
    </label>
  );
}

export interface RadioProps extends ChoiceBase {
  checked: boolean;
  onChange: () => void;
}

/** One option of a radio group (give every Radio of a group the same `name`; wrap them in a fieldset). */
export function Radio({ label, description, className, onChange, ref, ...rest }: RadioProps) {
  return (
    <Row
      kind="radio" label={label} description={description} className={className} mark={null}
      input={<input ref={ref} type="radio" className={s.input} onChange={() => onChange()} {...rest} />}
    />
  );
}

export interface CheckboxProps extends ChoiceBase {
  checked: boolean;
  onChange: (checked: boolean) => void;
  /** a mixed state (some of a set chosen) */
  indeterminate?: boolean;
}

export function Checkbox({ label, description, className, onChange, indeterminate, ref, ...rest }: CheckboxProps) {
  const own = useRef<HTMLInputElement | null>(null);
  useEffect(() => { if (own.current) own.current.indeterminate = !!indeterminate; }, [indeterminate]);
  const setRef = (el: HTMLInputElement | null) => {
    own.current = el;
    if (typeof ref === 'function') ref(el);
    else if (ref) (ref as { current: HTMLInputElement | null }).current = el;
  };
  const mark = (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      {indeterminate ? <path d="M3.5 7h7" /> : <path d="M3 7.4 5.8 10 11 4.2" />}
    </svg>
  );
  return (
    <Row
      kind="checkbox" label={label} description={description} className={className} mark={mark}
      input={<input ref={setRef} type="checkbox" className={s.input} onChange={(e) => onChange(e.target.checked)} {...rest} />}
    />
  );
}
