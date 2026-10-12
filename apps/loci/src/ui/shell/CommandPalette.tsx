import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from 'react';
import { createPortal } from 'react-dom';
import { t } from '@core/i18n/i18n';
import { Icon } from '../design/icons';
import { Kbd } from '../design/primitives/Kbd';
import { cx, useModalLayer, usePresence } from '../design/primitives/layer';
import { useUi } from '../store/ui';
import { COMMANDS, commandLabel, rankCommands, runCommand, type Command } from './commands';
import s from './CommandPalette.module.css';

/** Ctrl/Cmd+K: one field, every command, grouped while the query is empty. */
export function CommandPalette({ onAdd }: { onAdd: () => void }) {
  const open = useUi((st) => st.paletteOpen);
  const setOpen = useUi((st) => st.setPaletteOpen);
  const { mounted, state } = usePresence(open, '--d-fast');
  const panel = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const [query, setQuery] = useState('');
  const [active, setActive] = useState(0);
  const listId = useId();
  const close = () => setOpen(false);
  useModalLayer(panel, open && mounted, close, input);

  useEffect(() => { if (open) { setQuery(''); setActive(0); } }, [open]);
  const results = useMemo(() => rankCommands(COMMANDS, query, commandLabel), [query]);
  useEffect(() => { setActive(0); }, [query]);
  useEffect(() => {
    list.current?.querySelector<HTMLElement>(`[data-index="${active}"]`)?.scrollIntoView({ block: 'nearest' });
  }, [active]);

  if (!mounted) return null;

  const pick = (c: Command | undefined) => {
    if (!c) return;
    close();
    // let the palette's focus return first, so a command that moves focus keeps it
    window.setTimeout(() => runCommand(c.id, onAdd), 0);
  };
  const onKey = (e: KeyboardEvent) => {
    const n = results.length;
    if (e.key === 'ArrowDown') { e.preventDefault(); setActive((a) => (n ? (a + 1) % n : 0)); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); setActive((a) => (n ? (a - 1 + n) % n : 0)); }
    else if (e.key === 'Home' && !query) { e.preventDefault(); setActive(0); }
    else if (e.key === 'End' && !query) { e.preventDefault(); setActive(Math.max(0, n - 1)); }
    else if (e.key === 'Enter') { e.preventDefault(); pick(results[active]); }
  };

  const grouped = !query.trim();
  let lastGroup = '';
  return createPortal(
    <div className={s.root} data-state={state}>
      <div className={s.scrim} onClick={close} aria-hidden />
      <div ref={panel} role="dialog" aria-modal="true" aria-label={t('palette.title')} className={s.panel}>
        <div className={s.field}>
          <Icon name="search" size={18} className={s.fieldIcon} />
          <input
            ref={input}
            className={s.input}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onKey}
            placeholder={t('palette.placeholder')}
            role="combobox"
            aria-expanded="true"
            aria-controls={listId}
            aria-activedescendant={results[active] ? `${listId}-${results[active].id}` : undefined}
            aria-autocomplete="list"
            autoComplete="off"
            spellCheck={false}
          />
          <Kbd always className={s.esc}>esc</Kbd>
        </div>
        <ul ref={list} id={listId} role="listbox" aria-label={t('palette.title')} className={s.list}>
          {results.length === 0 && <li className={s.empty} role="presentation">{t('palette.none')}</li>}
          {results.map((c, i) => {
            const heading = grouped && c.group !== lastGroup ? c.group : null;
            lastGroup = c.group;
            return [
              heading && <li key={`h-${heading}`} role="presentation" className={s.group}>{t(heading)}</li>,
              <li
                key={c.id}
                id={`${listId}-${c.id}`}
                data-index={i}
                role="option"
                aria-selected={i === active}
                className={cx(s.option, i === active && s.active)}
                onMouseMove={() => { if (i !== active) setActive(i); }}
                onClick={() => pick(c)}
              >
                <Icon name={c.icon} size={18} className={s.optionIcon} />
                <span className={s.label}>{commandLabel(c)}</span>
                {c.id === 'manual' && <Icon name="external" size={14} className={s.external} />}
                {c.kbd && <Kbd className={s.kbd}>{c.kbd}</Kbd>}
              </li>,
            ];
          })}
        </ul>
      </div>
    </div>,
    document.body,
  );
}
