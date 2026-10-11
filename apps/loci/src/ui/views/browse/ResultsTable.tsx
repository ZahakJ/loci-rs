/**
 * Virtualised results grid (@tanstack/react-virtual): 36px rows, sticky header with sortable
 * columns, keyboard-first selection (j/k, arrows, shift to extend, ctrl+a, space toggles, Enter
 * opens, Delete deletes, Esc clears), shift-click ranges and ctrl-click toggles. On phones the
 * rows become two-line list items.
 */
import { useEffect, useLayoutEffect, useRef, type KeyboardEvent, type MouseEvent } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { t } from '@core/i18n/i18n';
import type { BrowseColumn, BrowseRow } from '@core/types';
import { Icon } from '@ui/design/icons';
import { cx } from '@ui/design/primitives/layer';
import { DATA_COLUMNS, NUMERIC, WIDTH } from './columns';
import type { SelectionAction, SelectionState } from './selection';
import type { BrowseData } from './useBrowseData';
import s from './ResultsTable.module.css';

export interface ResultsTableProps {
  data: BrowseData;
  columns: BrowseColumn[];
  sort: { column: BrowseColumn; descending: boolean };
  onSort: (column: BrowseColumn) => void;
  selection: SelectionState;
  dispatch: (a: SelectionAction) => void;
  onSelectAll: () => void;
  onOpen: (index: number) => void;
  onDelete: () => void;
  phone?: boolean;
  /** phone: taps toggle selection instead of opening */
  selectMode?: boolean;
  label: string;
  gridRef?: React.RefObject<HTMLDivElement | null>;
}

const ROW = 36;
const PHONE_ROW = 64;
const FLAG_CLASS = ['', 'f1', 'f2', 'f3', 'f4', 'f5', 'f6', 'f7'];

export function tableMinWidth(columns: BrowseColumn[]): string {
  const rem = columns.reduce((a, c) => a + parseFloat(/([\d.]+)rem/.exec(WIDTH[c])?.[1] ?? '6'), 0);
  return `calc(${rem}rem + 28px)`;
}

export function ResultsTable({ data, columns, sort, onSort, selection, dispatch, onSelectAll, onOpen, onDelete, phone, selectMode, label, gridRef }: ResultsTableProps) {
  const scroller = useRef<HTMLDivElement>(null);
  const own = useRef<HTMLDivElement>(null);
  const grid = gridRef ?? own;
  const rowH = phone ? PHONE_ROW : ROW;
  const virt = useVirtualizer({ count: data.total, getScrollElement: () => scroller.current, estimateSize: () => rowH, overscan: 12 });
  const items = virt.getVirtualItems();
  const first = items[0]?.index ?? 0;
  const last = items[items.length - 1]?.index ?? 0;
  const template = `28px ${columns.map((c) => WIDTH[c]).join(' ')}`;

  useEffect(() => { if (data.total) data.ensure(first, last); }, [first, last, data.total, data.version, data]);
  useLayoutEffect(() => { virt.measure(); }, [rowH, virt]);
  useEffect(() => {
    if (selection.cursor !== null) virt.scrollToIndex(selection.cursor, { align: 'auto' });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selection.cursor]);

  const ids = data.ids;
  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const mod = e.ctrlKey || e.metaKey;
    const k = e.key;
    const page = Math.max(1, Math.floor((scroller.current?.clientHeight ?? 400) / rowH) - 1);
    let delta = 0;
    if (k === 'j' || k === 'J' || k === 'ArrowDown') delta = 1;
    else if (k === 'k' || k === 'K' || k === 'ArrowUp') delta = -1;
    else if (k === 'PageDown') delta = page;
    else if (k === 'PageUp') delta = -page;
    else if (k === 'Home') delta = -ids.length;
    else if (k === 'End') delta = ids.length;
    if (delta && !mod && !e.altKey) { e.preventDefault(); dispatch({ type: 'move', delta, ids, extend: e.shiftKey }); return; }
    if (mod && k.toLowerCase() === 'a') { e.preventDefault(); onSelectAll(); return; }
    if (k === 'Escape' && selection.selected.size > 1) { e.preventDefault(); e.stopPropagation(); dispatch({ type: 'clear' }); return; }
    if (k === ' ' && selection.cursor !== null) { e.preventDefault(); dispatch({ type: 'click', index: selection.cursor, ids, toggle: true }); return; }
    if (k === 'Enter' && selection.cursor !== null) { e.preventDefault(); onOpen(selection.cursor); return; }
    if ((k === 'Delete' || (k === 'Backspace' && mod)) && selection.selected.size) { e.preventDefault(); onDelete(); }
  };

  const onRowClick = (e: MouseEvent, index: number) => {
    if (phone && !selectMode && !e.shiftKey && !e.ctrlKey && !e.metaKey) { dispatch({ type: 'click', index, ids }); onOpen(index); return; }
    dispatch({ type: 'click', index, ids, shift: e.shiftKey, toggle: e.ctrlKey || e.metaKey || (phone && selectMode) });
    grid.current?.focus({ preventScroll: true });
  };

  const rowId = (i: number) => `browse-row-${i}`;
  const cursor = selection.cursor;

  return (
    <div
      ref={grid}
      className={cx(s.grid, phone && s.phone)}
      role="grid"
      aria-label={label}
      aria-rowcount={data.total + 1}
      aria-colcount={columns.length + 1}
      aria-multiselectable="true"
      aria-activedescendant={cursor !== null && cursor >= first && cursor <= last ? rowId(cursor) : undefined}
      aria-busy={data.loading || undefined}
      tabIndex={0}
      onKeyDown={onKey}
      onFocus={(e) => { if (cursor === null && data.total > 0 && e.target === e.currentTarget && e.currentTarget.matches(':focus-visible')) dispatch({ type: 'move', delta: 1, ids }); }}
      style={{ '--cols': template, '--min': tableMinWidth(columns) } as React.CSSProperties}
    >
      <div ref={scroller} className={s.scroller}>
        {!phone && (
          <div role="row" aria-rowindex={1} className={cx(s.row, s.header)}>
            <span role="columnheader" className={s.lead}><span className="sr-only">{t('browse.state')}</span></span>
            {columns.map((c) => {
              const active = sort.column === c;
              return (
                <span key={c} role="columnheader" aria-sort={active ? (sort.descending ? 'descending' : 'ascending') : 'none'} className={cx(s.cell, NUMERIC.has(c) && s.num)}>
                  <button type="button" tabIndex={-1} className={cx(s.sortBtn, active && s.sorted)} onClick={() => onSort(c)}>
                    <span className={s.headLabel}>{t(`browse.columns.${c}`)}</span>
                    <Icon name={active && sort.descending ? 'arrow-down' : 'arrow-up'} size={13} className={s.sortIcon} />
                  </button>
                </span>
              );
            })}
          </div>
        )}
        <div className={s.body} style={{ blockSize: virt.getTotalSize() }}>
          {items.map((vi) => {
            const r = data.row(vi.index);
            const id = ids[vi.index];
            const selected = id !== undefined && selection.selected.has(id);
            return (
              <div
                key={vi.key}
                id={rowId(vi.index)}
                role="row"
                aria-rowindex={vi.index + 2}
                aria-selected={selected}
                data-cursor={vi.index === cursor || undefined}
                className={cx(s.row, selected && s.selected, r?.suspended && s.suspended, r?.buried && s.buried, !r && s.pending)}
                style={{ transform: `translateY(${vi.start}px)`, blockSize: rowH }}
                onClick={(e) => onRowClick(e, vi.index)}
                onDoubleClick={() => !phone && onOpen(vi.index)}
              >
                {phone ? <PhoneRow row={r} selected={selected} selectMode={!!selectMode} /> : (
                  <>
                    <span role="gridcell" className={cx(s.lead, r && FLAG_CLASS[r.flag] && s[FLAG_CLASS[r.flag]!])}>
                      <RowState row={r} />
                    </span>
                    {columns.map((c) => (
                      <span key={c} role="gridcell" className={cx(s.cell, NUMERIC.has(c) && s.num, c === 'sortField' && s.primary)} dir={c === 'sortField' || c === 'question' || c === 'answer' ? 'auto' : undefined}>
                        {r ? (DATA_COLUMNS.has(c) ? <span className="num">{r.cells[c] ?? ''}</span> : r.cells[c] ?? '') : <span className={s.ghost} />}
                      </span>
                    ))}
                  </>
                )}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

function RowState({ row }: { row: BrowseRow | undefined }) {
  if (!row) return null;
  if (row.suspended) return <Icon name="suspend" size={14} label={t('browse.rowSuspended')} />;
  if (row.buried) return <Icon name="bury" size={14} label={t('browse.rowBuried')} />;
  if (row.isLeech) return <Icon name="warning" size={14} label={t('browse.rowLeech')} />;
  if (row.flag) return <span className="sr-only">{t(`study.flags.${row.flag}`)}</span>;
  return null;
}

function PhoneRow({ row, selected, selectMode }: { row: BrowseRow | undefined; selected: boolean; selectMode: boolean }) {
  if (!row) return <span role="gridcell" className={s.phoneCell}><span className={s.ghost} /><span className={cx(s.ghost, s.ghostShort)} /></span>;
  const meta = [row.cells.deck, row.cells.due ?? row.cells.notetype].filter(Boolean).join(' · ');
  return (
    <span role="gridcell" className={cx(s.phoneCell, row.flag ? s[`pf${row.flag}`] : undefined)}>
      {selectMode && <span className={cx(s.check, selected && s.checked)} aria-hidden>{selected && <Icon name="check" size={14} />}</span>}
      <span className={s.phoneText}>
        <span className={s.phoneMain} dir="auto">{row.cells.sortField}</span>
        <span className={s.phoneMeta}>
          {row.suspended && <Icon name="suspend" size={13} label={t('browse.rowSuspended')} />}
          {row.isLeech && <Icon name="warning" size={13} label={t('browse.rowLeech')} />}
          {meta}
        </span>
      </span>
      {!selectMode && <Icon name="chevron-right" size={16} className={s.chev} />}
    </span>
  );
}
