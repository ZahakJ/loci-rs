/**
 * Browse (internals §9): sidebar · virtual table · editor on desktop; list → detail on phones.
 * The query lives in the URL hash (`#/browse?q=`) without remounting the view.
 */
import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from 'react';
import { t } from '@core/i18n/i18n';
import type { BrowseColumn, BrowseMode, BulkAction, DeckTreeNode, Flag } from '@core/types';
import { api, errorMessage, useQuery } from '@ui/api';
import { Button, IconButton } from '@ui/design/primitives/Button';
import { Dialog } from '@ui/design/primitives/Dialog';
import { EmptyState } from '@ui/design/primitives/EmptyState';
import { Field } from '@ui/design/primitives/Field';
import { Input } from '@ui/design/primitives/Input';
import { Segmented } from '@ui/design/primitives/Segmented';
import { Sheet } from '@ui/design/primitives/Sheet';
import { cx, isModalOpen, useMedia } from '@ui/design/primitives/layer';
import { toast } from '@ui/design/primitives/Toast';
import { isEditable } from '@ui/keys/keymap';
import { formatRoute } from '@ui/routes';
import { CardInfoSheet } from '../cardinfo/CardInfoPanel';
import { BulkBar, type DialogKind } from './BulkBar';
import { ColumnsMenu } from './ColumnsMenu';
import { DEFAULT_COLUMNS, readLayout, toggleColumn, writeLayout, type BrowseLayout } from './columns';
import { ResultsTable } from './ResultsTable';
import { RowEditor } from './RowEditor';
import { SearchBar } from './SearchBar';
import { emptySelection, selectionReducer } from './selection';
import { Sidebar } from './Sidebar';
import { useBrowseData } from './useBrowseData';
import s from './Browse.module.css';

function flattenDecks(node: DeckTreeNode | undefined): Array<{ id: number; name: string }> {
  const out: Array<{ id: number; name: string }> = [];
  const walk = (n: DeckTreeNode) => { for (const c of n.children) { out.push({ id: c.deck.id, name: c.deck.name }); walk(c); } };
  if (node) walk(node);
  return out;
}

/** Combines a sidebar search with the current one (ctrl-click = AND, alt-click = NOT). */
export function combineQuery(current: string, add: string, how: 'replace' | 'and' | 'not'): string {
  if (how === 'replace' || !current.trim()) return how === 'not' ? `-${add}` : add;
  return how === 'and' ? `${current.trim()} ${add}` : `${current.trim()} -${add}`;
}

export default function BrowseView({ initialQuery }: { initialQuery?: string }) {
  const phone = useMedia('(max-width: 719px)');
  const wide = useMedia('(min-width: 1280px)');
  const [layout, setLayout] = useState<BrowseLayout>(readLayout);
  const [query, setQuery] = useState(initialQuery ?? '');
  const [sel, dispatch] = useReducer(selectionReducer, emptySelection);
  const [detail, setDetail] = useState<number | null>(null);
  const [sidebarSheet, setSidebarSheet] = useState(false);
  const [selectMode, setSelectMode] = useState(false);
  const [saveOpen, setSaveOpen] = useState(false);
  const [saveName, setSaveName] = useState('');
  const [bulkDialog, setBulkDialog] = useState<DialogKind | null>(null);
  const [bulkPinned, setBulkPinned] = useState(false);
  const [infoCard, setInfoCard] = useState<number | null>(null);
  const gridRef = useRef<HTMLDivElement>(null);
  const mode = layout.mode;
  const sortCfg = layout.sort[mode];
  const sort = useMemo(() => ({ column: sortCfg.column, descending: sortCfg.descending }), [sortCfg.column, sortCfg.descending]);
  const columns = layout.columns[mode];
  const data = useBrowseData(query, mode, sort);
  const tree = useQuery((a) => a.deckTree(), [], ['decks']);
  const decks = useMemo(() => flattenDecks(tree.data), [tree.data]);
  const notetypes = useQuery((a) => a.listNotetypes(), [], ['notetypes']);
  const fieldNames = useMemo(() => [...new Set((notetypes.data ?? []).flatMap((n) => n.fields.map((f) => f.name)))], [notetypes.data]);

  useEffect(() => { document.title = `${t('browse.title')} · Loci`; }, []);
  useEffect(() => { writeLayout(layout); }, [layout]);
  useEffect(() => { dispatch({ type: 'reset' }); setDetail(null); }, [query, mode, sort]);
  useEffect(() => { dispatch({ type: 'prune', ids: data.ids }); }, [data.ids]);
  useEffect(() => { if (sel.selected.size === 0) setBulkPinned(false); }, [sel.selected.size]);

  const search = useCallback((q: string) => {
    setQuery(q);
    // keep the URL shareable without a hashchange (which would remount the view)
    const hash = formatRoute({ name: 'browse', q });
    if (window.location.hash !== hash) history.replaceState(history.state, '', `${location.pathname}${location.search}${hash}`);
  }, []);

  const setMode = (m: BrowseMode) => setLayout((l) => ({ ...l, mode: m }));
  const onSort = (c: BrowseColumn) => setLayout((l) => {
    const cur = l.sort[l.mode];
    return { ...l, sort: { ...l.sort, [l.mode]: { column: c, descending: cur.column === c ? !cur.descending : false } } };
  });

  const selectAll = async () => {
    try { dispatch({ type: 'selectAll', ids: (await api.searchIds(query, mode)) as number[] }); }
    catch (e) { toast({ message: errorMessage(e), tone: 'danger' }); }
  };

  /** Card ids for the current selection (notes mode resolves each note's cards). */
  const selectedCardIds = async (): Promise<number[]> => {
    const ids = [...sel.selected];
    if (mode === 'cards') return ids;
    if (ids.length === data.total && data.total > 0) return (await api.searchIds(query, 'cards')) as number[];
    if (ids.length <= 500) return (await Promise.all(ids.map((id) => api.getCardsOfNote(id)))).flat().map((c) => c.id);
    return (await api.searchIds(`nid:${ids.join(',')}`, 'cards')) as number[];
  };

  const runBulk = async (action: BulkAction) => {
    try {
      const ids = await selectedCardIds();
      if (!ids.length) return;
      const res = await api.bulk(ids, action);
      if (action.kind === 'delete') dispatch({ type: 'clear' });
      toast({
        message: `${res.undoLabel}: ${t('browse.bulk.done', { count: res.affected })}`,
        icon: 'check',
        action: { label: t('common.undo'), onClick: () => { void api.undo().then((r) => r && toast({ message: t('undo.done', { label: r.label }), icon: 'undo' })); } },
      });
    } catch (e) {
      toast({ message: errorMessage(e), tone: 'danger' });
    }
  };

  // Anki's browser shortcuts (when focus is not in a text field)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (isModalOpen() || e.defaultPrevented) return;
      const mod = e.ctrlKey || e.metaKey;
      if (!mod && !e.altKey && e.key === 'i' && !isEditable(document.activeElement)) {
        const r = phone ? (detail !== null ? data.row(detail) : undefined) : (sel.cursor !== null ? data.row(sel.cursor) : undefined);
        if (r) { e.preventDefault(); setInfoCard(r.cardId); }
        return;
      }
      if (!mod || sel.selected.size === 0) return;
      const inField = isEditable(document.activeElement);
      if (e.shiftKey && e.code === 'KeyD' && !inField) { e.preventDefault(); setBulkDialog('setDue'); }
      else if (e.altKey && e.code === 'KeyN' && !inField) { e.preventDefault(); setBulkDialog('forget'); }
      else if (e.altKey && e.code === 'KeyF') { e.preventDefault(); setBulkDialog('findReplace'); }
      else if (!e.shiftKey && !e.altKey && e.key === 'Delete' && !inField) { e.preventDefault(); setBulkDialog('delete'); }
      else if (!e.shiftKey && !e.altKey && e.code === 'KeyJ' && !inField) { e.preventDefault(); void runBulk({ kind: data.row(sel.cursor ?? 0)?.suspended ? 'unsuspend' : 'suspend' }); }
      else if (!e.shiftKey && !e.altKey && /^Digit[1-7]$/.test(e.code) && !inField) {
        e.preventDefault();
        const f = Number(e.code.slice(5)) as Flag;
        const cur = data.row(sel.cursor ?? 0)?.flag;
        void runBulk({ kind: 'flag', flag: cur === f ? 0 : f });
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  const deleteSaved = async (name: string) => {
    try { await api.deleteSavedSearch(name); toast({ message: t('browse.savedDeleted', { name }), icon: 'check' }); }
    catch (e) { toast({ message: errorMessage(e), tone: 'danger' }); }
  };
  const saveSearch = async () => {
    try { await api.saveSearch(saveName.trim(), query); setSaveOpen(false); toast({ message: t('browse.searchSaved', { name: saveName.trim() }), icon: 'star' }); }
    catch (e) { toast({ message: errorMessage(e), tone: 'danger' }); }
  };

  const cursorRow = sel.cursor !== null ? data.row(sel.cursor) : undefined;
  const editorRow = phone ? (detail !== null ? data.row(detail) : undefined) : cursorRow;
  const count = sel.selected.size;
  const noun = mode === 'notes' ? 'notes' : 'cards';
  const showSidebar = wide && layout.sidebar;
  const status = (
    <p className={s.status} aria-live="polite">
      <span className="num">{data.loading ? t('browse.searching') : t(mode === 'notes' ? 'browse.totalNotes' : 'browse.totalCards', { count: data.total })}</span>
      {count > 0 && <span className="num"> · {t('browse.selectedShort', { count })}</span>}
    </p>
  );

  const sidebar = (
    <Sidebar
      query={query}
      onSearch={(q, how) => { search(combineQuery(query, q, how)); setSidebarSheet(false); }}
      onDeleteSaved={(n) => void deleteSaved(n)}
      className={s.sidebar}
    />
  );

  const table = data.error ? (
    <div className={s.empty} role="alert">
      <EmptyState icon="search" compact title={t('browse.invalidTitle')} body={t('browse.invalidSearch', { message: data.error })} />
    </div>
  ) : !data.loading && data.total === 0 ? (
    <div className={s.empty}>
      <EmptyState icon="search" compact title={t('browse.results', { count: 0 })} body={query ? t('browse.noMatchesBody') : t('browse.emptyCollection')}
        actions={query ? <Button size="sm" onClick={() => search('')}>{t('browse.showAll')}</Button> : undefined} />
    </div>
  ) : (
    <ResultsTable
      data={data}
      columns={columns}
      sort={sort}
      onSort={onSort}
      selection={sel}
      dispatch={dispatch}
      onSelectAll={() => void selectAll()}
      onOpen={(i) => { if (phone) setDetail(i); else document.querySelector<HTMLElement>('[data-row-editor] [contenteditable]')?.focus(); }}
      onDelete={() => setBulkDialog('delete')}
      phone={phone}
      selectMode={selectMode}
      label={t('browse.tableLabel')}
      gridRef={gridRef}
    />
  );

  const bulkVisible = phone ? selectMode && count > 0 : count > 1 || (count === 1 && bulkPinned) || bulkDialog !== null;
  const bulk = bulkVisible ? (
    <BulkBar
      count={count}
      noun={noun}
      onRun={runBulk}
      onClear={() => { dispatch({ type: 'clear' }); setBulkPinned(false); gridRef.current?.focus(); }}
      decks={decks}
      fieldNames={fieldNames}
      onInfo={count === 1 && cursorRow ? () => setInfoCard(cursorRow.cardId) : undefined}
      className={s.bulk}
      dialog={bulkDialog}
      onDialogChange={setBulkDialog}
    />
  ) : null;

  // ── phone: list → detail ──
  if (phone) {
    if (detail !== null && editorRow) {
      return (
        <div className={cx(s.root, s.phoneRoot)}>
          <h1 className="sr-only" data-page-title tabIndex={-1}>{t('browse.title')}</h1>
          <RowEditor key={editorRow.noteId} noteId={editorRow.noteId} cardId={editorRow.cardId} deckName={editorRow.cells.deck} onBack={() => setDetail(null)} onInfo={() => setInfoCard(editorRow.cardId)} className={s.detail} />
          <CardInfoSheet open={infoCard !== null} onClose={() => setInfoCard(null)} cardId={infoCard} />
        </div>
      );
    }
    return (
      <div className={cx(s.root, s.phoneRoot)}>
        <h1 className="sr-only" data-page-title tabIndex={-1}>{t('browse.title')}</h1>
        <div className={s.phoneTop}>
          <SearchBar value={query} onSearch={search} onEscape={() => gridRef.current?.focus()} busy={data.fetching && !data.loading} placeholder={t('browse.searchShort')} />
          <IconButton icon="sidebar" label={t('browse.sidebar.open')} variant="secondary" onClick={() => setSidebarSheet(true)} />
        </div>
        <div className={s.phoneTools}>
          <Segmented label={t('browse.mode')} value={mode} onChange={setMode} options={[{ value: 'cards', label: t('browse.modeCards') }, { value: 'notes', label: t('browse.modeNotes') }]} />
          {status}
          <Button size="sm" variant={selectMode ? 'secondary' : 'ghost'} aria-pressed={selectMode} onClick={() => { setSelectMode((m) => !m); dispatch({ type: 'clear' }); }}>
            {selectMode ? t('common.done') : t('browse.select')}
          </Button>
        </div>
        <div className={s.tableWrap}>{table}</div>
        {bulk && <div className={s.phoneBulk}>{bulk}</div>}
        <CardInfoSheet open={infoCard !== null} onClose={() => setInfoCard(null)} cardId={infoCard} />
        <Sheet open={sidebarSheet} onClose={() => setSidebarSheet(false)} title={t('browse.sidebar.label')} phone="bottom" closeLabel={t('common.close')} bodyClassName={s.sheetBody}>
          {sidebar}
        </Sheet>
      </div>
    );
  }

  return (
    <div className={cx(s.root, showSidebar && s.withSidebar, editorRow && s.withEditor)}>
      <h1 className="sr-only" data-page-title tabIndex={-1}>{t('browse.title')}</h1>
      {showSidebar && <aside className={s.sidePane}>{sidebar}</aside>}
      <section className={s.center} aria-label={t('browse.results', { count: data.total })}>
        <div className={s.top}>
          <IconButton
            icon="sidebar"
            label={t(showSidebar ? 'browse.sidebar.hide' : 'browse.sidebar.open')}
            aria-pressed={showSidebar}
            onClick={() => (wide ? setLayout((l) => ({ ...l, sidebar: !l.sidebar })) : setSidebarSheet(true))}
          />
          <SearchBar value={query} onSearch={search} onEscape={() => gridRef.current?.focus()} onSave={() => { setSaveName(''); setSaveOpen(true); }} busy={data.fetching && !data.loading} />
        </div>
        <div className={s.tools}>
          {bulk ?? (
            <>
              <Segmented label={t('browse.mode')} value={mode} onChange={setMode} options={[{ value: 'cards', label: t('browse.modeCards') }, { value: 'notes', label: t('browse.modeNotes') }]} />
              {status}
              {count === 1 && <Button size="sm" variant="ghost" icon="more" onClick={() => setBulkPinned(true)}>{t('browse.actions')}</Button>}
              <ColumnsMenu mode={mode} columns={columns} onToggle={(c) => setLayout((l) => ({ ...l, columns: { ...l.columns, [l.mode]: toggleColumn(l.columns[l.mode], c) } }))} onReset={() => setLayout((l) => ({ ...l, columns: { ...l.columns, [l.mode]: DEFAULT_COLUMNS[l.mode] } }))} />
            </>
          )}
        </div>
        <div className={s.tableWrap}>{table}</div>
        {bulk && <div className={s.footStatus}>{status}</div>}
      </section>
      {editorRow && (
        <aside className={s.editorPane} data-row-editor="">
          <RowEditor key={`${editorRow.noteId}`} noteId={editorRow.noteId} cardId={editorRow.cardId} deckName={editorRow.cells.deck} onInfo={() => setInfoCard(editorRow.cardId)} />
        </aside>
      )}
      <Sheet open={sidebarSheet} onClose={() => setSidebarSheet(false)} title={t('browse.sidebar.label')} width="20rem" closeLabel={t('common.close')} bodyClassName={s.sheetBody}>
        {sidebar}
      </Sheet>
      <CardInfoSheet open={infoCard !== null} onClose={() => setInfoCard(null)} cardId={infoCard} />
      <Dialog open={saveOpen} onClose={() => setSaveOpen(false)} title={t('browse.saveSearch')} description={<code className={s.code}>{query}</code>}
        confirmLabel={t('common.save')} confirmDisabled={!saveName.trim()} onConfirm={() => void saveSearch()}>
        <Field label={t('browse.savedSearchName')}><Input value={saveName} onChange={(e) => setSaveName(e.target.value)} data-autofocus /></Field>
      </Dialog>
    </div>
  );
}
