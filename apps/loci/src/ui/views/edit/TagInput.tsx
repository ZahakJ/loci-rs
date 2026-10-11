/**
 * Tag editor: chips plus a text box with autocomplete from the collection's tags. Tags cannot
 * contain spaces (Anki); space, Enter or comma commits; Backspace on an empty box edits the last
 * chip. Hierarchical tags (`a::b`) match on any segment and show their path muted.
 */
import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from 'react';
import { t } from '@core/i18n/i18n';
import { useQuery } from '@ui/api';
import { Icon } from '@ui/design/icons';
import { cx } from '@ui/design/primitives/layer';
import s from './TagInput.module.css';

export interface TagInputProps {
  tags: string[];
  onChange: (tags: string[]) => void;
  label?: string;
  hideLabel?: boolean;
  placeholder?: string;
  onSubmit?: () => void;
  className?: string;
  autoFocus?: boolean;
}

/** Suggestions for a partial tag: prefix of the whole tag first, then of any `::` segment. */
export function suggestTags(all: Array<{ tag: string; count: number }>, query: string, exclude: string[], limit = 8): Array<{ tag: string; count: number }> {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  const ex = new Set(exclude.map((x) => x.toLowerCase()));
  const scored = all
    .filter((x) => !ex.has(x.tag.toLowerCase()))
    .map((x) => {
      const tag = x.tag.toLowerCase();
      const score = tag.startsWith(q) ? 0 : tag.split('::').some((seg) => seg.startsWith(q)) ? 1 : tag.includes(q) ? 2 : -1;
      return { x, score };
    })
    .filter((y) => y.score >= 0)
    .sort((a, b) => a.score - b.score || b.x.count - a.x.count || a.x.tag.localeCompare(b.x.tag));
  return scored.slice(0, limit).map((y) => y.x);
}

export function TagInput({ tags, onChange, label, hideLabel, placeholder, onSubmit, className, autoFocus }: TagInputProps) {
  const id = useId();
  const [text, setText] = useState('');
  const [active, setActive] = useState(-1);
  const [focused, setFocused] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const all = useQuery((a) => a.allTags(), [], ['tags', 'notes']);
  const suggestions = useMemo(() => suggestTags(all.data ?? [], text, tags), [all.data, text, tags]);
  const open = focused && suggestions.length > 0;
  // Esc closes the suggestions only (runs before the sheet's capture-phase Escape handler)
  useEffect(() => {
    if (!open) return;
    const onKey = (e: globalThis.KeyboardEvent) => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); setText(''); setActive(-1); } };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [open]);

  const commit = (raw: string) => {
    const parts = raw.split(/[\s,]+/).map((x) => x.trim()).filter(Boolean);
    if (!parts.length) return;
    const lower = new Set(tags.map((x) => x.toLowerCase()));
    const next = [...tags];
    for (const p of parts) if (!lower.has(p.toLowerCase())) { next.push(p); lower.add(p.toLowerCase()); }
    onChange(next);
    setText('');
    setActive(-1);
  };

  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') { commit(text); onSubmit?.(); e.preventDefault(); return; }
    if (e.key === 'ArrowDown' && open) { e.preventDefault(); setActive((i) => Math.min(suggestions.length - 1, i + 1)); return; }
    if (e.key === 'ArrowUp' && open) { e.preventDefault(); setActive((i) => Math.max(-1, i - 1)); return; }
    if (e.key === 'Enter' || e.key === ',' || e.key === ' ') {
      const pick = active >= 0 ? suggestions[active]?.tag : undefined;
      if (pick || text.trim()) { e.preventDefault(); commit(pick ?? text); }
      return;
    }
    if (e.key === 'Tab' && !e.shiftKey && active >= 0 && suggestions[active]) { e.preventDefault(); commit(suggestions[active]!.tag); return; }
    if (e.key === 'Backspace' && text === '' && tags.length) {
      e.preventDefault();
      const last = tags[tags.length - 1]!;
      onChange(tags.slice(0, -1));
      setText(last);
    }
  };

  const listId = `${id}-list`;
  return (
    <div className={cx(s.root, className)}>
      {label && <label htmlFor={id} className={cx(s.label, hideLabel && 'sr-only')}>{label}</label>}
      <div className={s.box} onClick={(e) => { if (e.target === e.currentTarget) input.current?.focus(); }}>
        <Icon name="tag" size={16} className={s.icon} />
        <ul role="list" className={s.chips} aria-label={t('editor.tagsCurrent')}>
          {tags.map((tag) => {
            const parts = tag.split('::');
            const leaf = parts.pop()!;
            return (
              <li key={tag} className={s.chip}>
                <span className={s.chipText}>{parts.length > 0 && <span className={s.chipPath}>{parts.join('::')}::</span>}{leaf}</span>
                <button type="button" className={s.remove} aria-label={t('editor.removeTag', { tag })} onClick={() => onChange(tags.filter((x) => x !== tag))}>
                  <Icon name="close" size={12} />
                </button>
              </li>
            );
          })}
        </ul>
        <input
          ref={input}
          id={id}
          className={s.input}
          value={text}
          placeholder={tags.length ? '' : placeholder}
          role="combobox"
          aria-expanded={open}
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={open && active >= 0 ? `${id}-s${active}` : undefined}
          autoComplete="off"
          autoCapitalize="off"
          spellCheck={false}
          autoFocus={autoFocus}
          onChange={(e) => {
            const v = e.target.value;
            if (/[\s,]/.test(v)) commit(v); else { setText(v); setActive(-1); }
          }}
          onKeyDown={onKey}
          onFocus={() => setFocused(true)}
          onBlur={() => { setFocused(false); if (text.trim()) commit(text); }}
        />
      </div>
      <ul id={listId} role="listbox" aria-label={t('editor.tagSuggestions')} className={s.suggest} hidden={!open}>
        {suggestions.map((sg, i) => (
          <li
            key={sg.tag}
            id={`${id}-s${i}`}
            role="option"
            aria-selected={i === active}
            className={s.option}
            onPointerDown={(e) => { e.preventDefault(); commit(sg.tag); }}
            onPointerMove={() => setActive(i)}
          >
            <span className={s.optTag}>{sg.tag}</span>
            <span className={s.optCount}>{sg.count.toLocaleString()}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
