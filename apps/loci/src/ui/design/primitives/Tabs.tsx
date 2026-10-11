import { useId, useRef, type KeyboardEvent, type ReactNode } from 'react';
import { cx } from './layer';
import s from './Tabs.module.css';

export interface TabItem<V extends string> { value: V; label: ReactNode; count?: number; content?: ReactNode; disabled?: boolean }
export interface TabsProps<V extends string> {
  items: Array<TabItem<V>>;
  value: V;
  onChange: (value: V) => void;
  label: string;
  className?: string;
  panelClassName?: string;
  /** manual activation: arrows move focus, Enter/Space selects (default: automatic) */
  manual?: boolean;
}

/** Tablist with arrow-key navigation; renders the active item's `content` in a tabpanel when given. */
export function Tabs<V extends string>({ items, value, onChange, label, className, panelClassName, manual }: TabsProps<V>) {
  const base = useId();
  const refs = useRef<Array<HTMLButtonElement | null>>([]);
  const onKey = (e: KeyboardEvent, i: number) => {
    const rtl = document.documentElement.dir === 'rtl';
    let next = -1;
    if (e.key === (rtl ? 'ArrowLeft' : 'ArrowRight')) next = (i + 1) % items.length;
    else if (e.key === (rtl ? 'ArrowRight' : 'ArrowLeft')) next = (i - 1 + items.length) % items.length;
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = items.length - 1;
    if (next < 0) return;
    e.preventDefault();
    refs.current[next]?.focus();
    if (!manual && !items[next]!.disabled) onChange(items[next]!.value);
  };
  const active = items.find((i) => i.value === value);
  return (
    <>
      <div role="tablist" aria-label={label} className={cx(s.list, className)}>
        {items.map((it, i) => (
          <button
            key={it.value}
            ref={(el) => { refs.current[i] = el; }}
            type="button"
            role="tab"
            id={`${base}-tab-${it.value}`}
            aria-selected={it.value === value}
            aria-controls={it.content !== undefined ? `${base}-panel-${it.value}` : undefined}
            tabIndex={it.value === value ? 0 : -1}
            disabled={it.disabled}
            className={s.tab}
            onClick={() => onChange(it.value)}
            onKeyDown={(e) => onKey(e, i)}
          >
            {it.label}
            {it.count !== undefined && <span className={s.count}>{it.count.toLocaleString()}</span>}
          </button>
        ))}
      </div>
      {active?.content !== undefined && (
        <div role="tabpanel" id={`${base}-panel-${active.value}`} aria-labelledby={`${base}-tab-${active.value}`} tabIndex={0} className={cx(s.panel, panelClassName)}>
          {active.content}
        </div>
      )}
    </>
  );
}
