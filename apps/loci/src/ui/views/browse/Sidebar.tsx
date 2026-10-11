/**
 * Browse sidebar (DESIGN §11): saved searches, Today, flags, decks, note types, tags, from
 * `api.sidebar()`. A tree (role=tree) with arrow-key navigation; Enter / click runs the item's
 * search, ctrl/cmd-click adds it to the current search, alt-click excludes it (as in Anki).
 */
import { useMemo, useRef, useState, type KeyboardEvent, type MouseEvent } from 'react';
import { t } from '@core/i18n/i18n';
import type { SidebarItem } from '@core/types';
import { useQuery } from '@ui/api';
import { Icon, type IconName } from '@ui/design/icons';
import { cx } from '@ui/design/primitives/layer';
import { Skeleton } from '@ui/design/primitives/Skeleton';
import s from './Sidebar.module.css';

export interface SidebarProps {
  query: string;
  onSearch: (query: string, how: 'replace' | 'and' | 'not') => void;
  onDeleteSaved?: (name: string) => void;
  className?: string;
}

const SECTION_ICON: Record<SidebarItem['kind'], IconName> = { saved: 'star', today: 'calendar', flag: 'flag', deck: 'deck', notetype: 'cards', tag: 'tag' };
const SECTION_LABEL: Record<SidebarItem['kind'], string> = {
  saved: 'browse.sidebar.saved', today: 'browse.sidebar.today', flag: 'browse.sidebar.flags', deck: 'browse.sidebar.decks', notetype: 'browse.sidebar.notetypes', tag: 'browse.sidebar.tags',
};
const FLAG_TONE = ['', 'again', 'hard', 'good', 'easy', 'pink', 'turquoise', 'purple'];

interface Flat { key: string; item: SidebarItem; depth: number; parent: string | null; hasChildren: boolean; section: boolean }

/** Groups `a::b::c` tags into a tree (the API returns them flat). */
export function tagTree(items: SidebarItem[]): SidebarItem[] {
  const root: SidebarItem[] = [];
  const byPath = new Map<string, SidebarItem>();
  for (const it of [...items].sort((a, b) => a.label.localeCompare(b.label))) {
    const parts = it.label.split('::');
    let level = root;
    let path = '';
    parts.forEach((part, i) => {
      path = path ? `${path}::${part}` : part;
      let node = byPath.get(path);
      if (!node) {
        node = { kind: 'tag', label: part, query: `tag:${path}`, children: [] };
        byPath.set(path, node);
        level.push(node);
      }
      if (i === parts.length - 1) { node.count = it.count; node.query = `tag:${path}`; }
      level = node.children!;
    });
  }
  return root;
}

export function Sidebar({ query, onSearch, onDeleteSaved, className }: SidebarProps) {
  const data = useQuery((a) => a.sidebar(), [], ['decks', 'notetypes', 'tags', 'notes', 'cards']);
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  const [focusKey, setFocusKey] = useState<string | null>(null);
  const tree = useRef<HTMLUListElement>(null);

  const sections = useMemo(() => (data.data ?? []).map((sec) => (sec.kind === 'tag' ? { ...sec, children: tagTree(sec.children ?? []) } : sec)), [data.data]);

  const flat = useMemo(() => {
    const out: Flat[] = [];
    const walk = (items: SidebarItem[], depth: number, parent: string | null, prefix: string) => {
      items.forEach((it, i) => {
        const key = `${prefix}/${i}:${it.label}`;
        const hasChildren = !!it.children?.length;
        out.push({ key, item: it, depth, parent, hasChildren, section: depth === 0 });
        if (hasChildren && !collapsed.has(key)) walk(it.children!, depth + 1, key, key);
      });
    };
    walk(sections, 0, null, '');
    return out;
  }, [sections, collapsed]);

  const toggle = (key: string) => setCollapsed((c) => { const n = new Set(c); if (n.has(key)) n.delete(key); else n.add(key); return n; });
  const activeKey = focusKey && flat.some((f) => f.key === focusKey) ? focusKey : flat[0]?.key ?? null;

  const run = (f: Flat, e?: MouseEvent | KeyboardEvent) => {
    if (f.section || !f.item.query) { if (f.hasChildren) toggle(f.key); return; }
    const how = e && (e.altKey) ? 'not' : e && (e.ctrlKey || e.metaKey) ? 'and' : 'replace';
    onSearch(f.item.query, how);
  };

  const focusRow = (key: string) => {
    setFocusKey(key);
    requestAnimationFrame(() => tree.current?.querySelector<HTMLElement>(`[data-key="${CSS.escape(key)}"]`)?.focus());
  };

  const onKey = (e: KeyboardEvent<HTMLLIElement>, f: Flat, i: number) => {
    const rtl = document.documentElement.dir === 'rtl';
    const open = rtl ? 'ArrowLeft' : 'ArrowRight', close = rtl ? 'ArrowRight' : 'ArrowLeft';
    if (e.key === 'ArrowDown') { e.preventDefault(); const n = flat[i + 1]; if (n) focusRow(n.key); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); const n = flat[i - 1]; if (n) focusRow(n.key); }
    else if (e.key === 'Home') { e.preventDefault(); if (flat[0]) focusRow(flat[0].key); }
    else if (e.key === 'End') { e.preventDefault(); const n = flat[flat.length - 1]; if (n) focusRow(n.key); }
    else if (e.key === open) {
      e.preventDefault();
      if (f.hasChildren && collapsed.has(f.key)) toggle(f.key);
      else if (f.hasChildren) { const n = flat[i + 1]; if (n) focusRow(n.key); }
    } else if (e.key === close) {
      e.preventDefault();
      if (f.hasChildren && !collapsed.has(f.key)) toggle(f.key);
      else if (f.parent) focusRow(f.parent);
    } else if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); run(f, e); }
    else if (e.key === 'Delete' && f.item.kind === 'saved' && !f.section && onDeleteSaved) { e.preventDefault(); onDeleteSaved(f.item.label); }
  };

  if (data.loading) {
    return (
      <nav className={cx(s.sidebar, className)} aria-label={t('browse.sidebar.label')} aria-busy="true">
        <div className={s.loading}>{Array.from({ length: 8 }, (_, i) => <Skeleton key={i} height="1.25rem" width={`${60 + ((i * 17) % 35)}%`} />)}</div>
      </nav>
    );
  }

  return (
    <nav className={cx(s.sidebar, className)} aria-label={t('browse.sidebar.label')}>
      <ul ref={tree} role="tree" aria-label={t('browse.sidebar.label')} className={s.tree}>
        {flat.map((f, i) => {
          const isActive = !f.section && !!f.item.query && f.item.query === query.trim();
          const expanded = f.hasChildren ? !collapsed.has(f.key) : undefined;
          const label = f.section ? t(SECTION_LABEL[f.item.kind]) : f.item.label;
          return (
            <li
              key={f.key}
              data-key={f.key}
              role="treeitem"
              aria-level={f.depth + 1}
              aria-expanded={expanded}
              aria-selected={isActive}
              tabIndex={f.key === activeKey ? 0 : -1}
              className={cx(s.row, f.section && s.section, isActive && s.active)}
              style={{ '--depth': f.depth } as React.CSSProperties}
              onClick={(e) => { setFocusKey(f.key); run(f, e); }}
              onKeyDown={(e) => onKey(e, f, i)}
              onFocus={() => setFocusKey(f.key)}
            >
              {f.hasChildren ? (
                <span className={s.twisty} aria-hidden onClick={(e) => { e.stopPropagation(); toggle(f.key); }}>
                  <Icon name="chevron-down" size={14} />
                </span>
              ) : <span className={s.twisty} aria-hidden />}
              {f.section ? <Icon name={SECTION_ICON[f.item.kind]} size={16} className={s.secIcon} />
                : f.item.kind === 'flag' ? <span className={cx(s.flagDot, s[`flag-${FLAG_TONE[Number(f.item.query.split(':')[1])]}`])} aria-hidden />
                : null}
              <span className={s.label} dir={f.item.kind === 'tag' || f.item.kind === 'deck' ? 'auto' : undefined}>{label}</span>
              {f.item.count !== undefined && f.item.count > 0 && <span className={s.count}>{f.item.count.toLocaleString()}</span>}
              {f.item.kind === 'saved' && !f.section && onDeleteSaved && (
                <button type="button" tabIndex={-1} className={s.del} aria-label={t('browse.deleteSavedSearch')} onClick={(e) => { e.stopPropagation(); onDeleteSaved(f.item.label); }}>
                  <Icon name="close" size={14} />
                </button>
              )}
            </li>
          );
        })}
      </ul>
      <p className={s.tip}>{t('browse.sidebar.tip')}</p>
    </nav>
  );
}
