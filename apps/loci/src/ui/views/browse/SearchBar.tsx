/**
 * Browse search box: Anki syntax, searches as you type (300 ms after the last keystroke; Enter
 * searches at once), `/` focuses (global key), Esc returns focus to the table. A syntax popover
 * lists examples; choosing one appends it to the search.
 */
import { useEffect, useId, useRef, useState, type KeyboardEvent } from 'react';
import { t } from '@core/i18n/i18n';
import { Icon } from '@ui/design/icons';
import { Kbd } from '@ui/design/primitives/Kbd';
import { cx } from '@ui/design/primitives/layer';
import s from './SearchBar.module.css';

/** Delay between the last keystroke and the live search. */
export const SEARCH_DEBOUNCE_MS = 300;

export interface SearchBarProps {
  value: string;
  onSearch: (q: string) => void;
  onEscape?: () => void;
  onSave?: () => void;
  busy?: boolean;
  placeholder?: string;
  className?: string;
}

export const SYNTAX: Array<{ q: string; key: string }> = [
  { q: 'deck:French', key: 'deck' },
  { q: 'tag:verbs', key: 'tag' },
  { q: 'note:Cloze', key: 'note' },
  { q: 'is:due', key: 'isDue' },
  { q: 'is:new', key: 'isNew' },
  { q: 'is:suspended', key: 'isSuspended' },
  { q: 'flag:1', key: 'flag' },
  { q: 'prop:ivl>21', key: 'propIvl' },
  { q: 'prop:r<0.8', key: 'propR' },
  { q: 'rated:7:1', key: 'rated' },
  { q: 'added:3', key: 'added' },
  { q: '"exact phrase"', key: 'phrase' },
  { q: '-tag:leech', key: 'not' },
  { q: 'dog OR cat', key: 'or' },
  { q: 'front:*hello*', key: 'field' },
  { q: 're:\\d{3}', key: 're' },
];

export function SearchBar({ value, onSearch, onEscape, onSave, busy, placeholder, className }: SearchBarProps) {
  const [text, setText] = useState(value);
  const [help, setHelp] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const pop = useRef<HTMLDivElement>(null);
  const helpBtn = useRef<HTMLButtonElement>(null);
  const popId = useId();
  const timer = useRef<number | undefined>(undefined);
  const textRef = useRef(text);
  textRef.current = text;
  const searchRef = useRef(onSearch);
  searchRef.current = onSearch;
  // An outside change of the query (sidebar, URL) replaces the text; our own live search coming
  // back must not (it would eat a trailing space while the user is still typing).
  useEffect(() => { if (value !== textRef.current.trim()) setText(value); }, [value]);
  useEffect(() => () => window.clearTimeout(timer.current), []);

  const pending = text.trim() !== value.trim();
  const change = (next: string) => {
    setText(next);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      if (next.trim() !== textRef.current.trim()) return;
      searchRef.current(next.trim());
    }, SEARCH_DEBOUNCE_MS);
  };

  useEffect(() => {
    if (!help) return;
    const onDown = (e: PointerEvent) => {
      const tg = e.target as Node;
      if (pop.current?.contains(tg) || helpBtn.current?.contains(tg)) return;
      setHelp(false);
    };
    const onKey = (e: globalThis.KeyboardEvent) => { if (e.key === 'Escape') { e.preventDefault(); setHelp(false); helpBtn.current?.focus(); } };
    document.addEventListener('pointerdown', onDown, true);
    window.addEventListener('keydown', onKey, true);
    return () => { document.removeEventListener('pointerdown', onDown, true); window.removeEventListener('keydown', onKey, true); };
  }, [help]);

  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') { e.preventDefault(); window.clearTimeout(timer.current); onSearch(text.trim()); }
    else if (e.key === 'Escape') {
      e.preventDefault();
      window.clearTimeout(timer.current);
      if (pending) setText(value);
      else onEscape?.();
    }
  };

  const insert = (q: string) => {
    const next = text.trim() ? `${text.trim()} ${q}` : q;
    change(next);
    setHelp(false);
    requestAnimationFrame(() => { input.current?.focus(); input.current?.setSelectionRange(next.length, next.length); });
  };

  return (
    <div className={cx(s.wrap, className)} role="search">
      <div className={s.box}>
        <Icon name="search" size={18} className={s.icon} />
        <input
          ref={input}
          type="search"
          data-global-search=""
          className={s.input}
          value={text}
          placeholder={placeholder ?? t('browse.searchPlaceholder')}
          aria-label={t('browse.searchLabel')}
          spellCheck={false}
          autoComplete="off"
          autoCapitalize="off"
          enterKeyHint="search"
          onChange={(e) => change(e.target.value)}
          onKeyDown={onKey}
        />
        {(busy || pending) && <span className={s.spinner} aria-hidden />}
        {!text && <span className={s.slash} aria-hidden><Kbd>/</Kbd></span>}
        {onSave && value.trim() && (
          <button type="button" className={s.iconBtn} aria-label={t('browse.saveSearch')} title={t('browse.saveSearch')} onClick={onSave}>
            <Icon name="star" size={16} />
          </button>
        )}
        <button
          ref={helpBtn}
          type="button"
          className={s.iconBtn}
          aria-label={t('browse.searchHelp')}
          title={t('browse.searchHelp')}
          aria-expanded={help}
          aria-controls={help ? popId : undefined}
          onClick={() => setHelp((h) => !h)}
        >
          <Icon name="help" size={16} />
        </button>
      </div>
      {help && (
        <div ref={pop} id={popId} className={s.pop} role="dialog" aria-label={t('browse.searchHelp')}>
          <p className={s.popTitle}>{t('browse.syntax.title')}</p>
          <ul role="list" className={s.examples}>
            {SYNTAX.map((x) => (
              <li key={x.q}>
                <button type="button" className={s.example} onClick={() => insert(x.q)}>
                  <code className={s.code}>{x.q}</code>
                  <span className={s.desc}>{t(`browse.syntax.${x.key}`)}</span>
                </button>
              </li>
            ))}
          </ul>
          <p className={s.popFoot}>{t('browse.syntax.foot')}</p>
        </div>
      )}
    </div>
  );
}
