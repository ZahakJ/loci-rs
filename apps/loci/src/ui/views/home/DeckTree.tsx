import { useRef, useState, type CSSProperties, type KeyboardEvent } from 'react';
import { t } from '@core/i18n/i18n';
import { stripHtml } from '@core/text';
import type { DeckTreeNode } from '@core/types';
import { Icon } from '../../design/icons';
import { Button, IconButton } from '../../design/primitives/Button';
import { Card } from '../../design/primitives/Card';
import { cx } from '../../design/primitives/layer';
import { Menu, type MenuEntry } from '../../design/primitives/Menu';
import { formatCount } from '../../hooks/format';
import { navigate } from '../../routes';
import { ExportDialog } from '../edit/ExportDialog';
import { DeckDialogs, type DeckDialogState } from './DeckDialogs';
import s from './DeckTree.module.css';

export interface DeckTreeProps {
  root: DeckTreeNode;
  onToggle: (node: DeckTreeNode) => void;
}

const leaf = (name: string) => name.split('::').pop() ?? name;

function Count({ n, tone, label }: { n: number; tone: 'new' | 'learn' | 'due'; label: string }) {
  return (
    <span className={cx(s.n, n === 0 ? s.zero : s[tone])} aria-label={label}>
      {n === 0 ? '–' : formatCount(n)}
    </span>
  );
}

export function DeckTree({ root, onToggle }: DeckTreeProps) {
  const [dialog, setDialog] = useState<DeckDialogState>(null);
  const listRef = useRef<HTMLUListElement>(null);

  // The last deck exported stays set while the dialog closes, so its content does not vanish mid-animation.
  const [exporting, setExporting] = useState<{ node: DeckTreeNode; open: boolean } | null>(null);

  const menuFor = (node: DeckTreeNode): MenuEntry[] => [
    { id: 'study', label: t('deck.study'), icon: 'play', onSelect: () => navigate(`#/study/${node.deck.id}`) },
    { id: 'stats', label: t('deck.stats'), icon: 'stats', onSelect: () => navigate(`#/stats/${node.deck.id}`) },
    { id: 'browse', label: t('deck.browse'), icon: 'search', onSelect: () => navigate(`#/browse?q=${encodeURIComponent(`deck:"${node.deck.name}"`)}`) },
    { separator: true, id: 's1' },
    { id: 'rename', label: t('deck.rename'), icon: 'edit', onSelect: () => setDialog({ kind: 'rename', node }) },
    { id: 'options', label: t('deck.options'), icon: 'settings', onSelect: () => setDialog({ kind: 'options', node }) },
    { id: 'description', label: t('deck.description'), icon: 'text', onSelect: () => setDialog({ kind: 'description', node }) },
    { id: 'export', label: t('deck.export'), icon: 'export', onSelect: () => setExporting({ node, open: true }) },
    { separator: true, id: 's2' },
    { id: 'delete', label: t('deck.delete'), icon: 'trash', danger: true, onSelect: () => setDialog({ kind: 'delete', node }) },
  ];

  // Arrow keys move between deck rows; Left/Right collapse and expand.
  const onKeyDown = (e: KeyboardEvent<HTMLUListElement>) => {
    const target = e.target as HTMLElement;
    if (!target.matches('[data-deck-link]')) return;
    const links = Array.from(listRef.current?.querySelectorAll<HTMLElement>('[data-deck-link]') ?? []).filter((l) => l.getClientRects().length > 0);
    const i = links.indexOf(target);
    const rtl = document.documentElement.dir === 'rtl';
    if (e.key === 'ArrowDown') { e.preventDefault(); links[Math.min(links.length - 1, i + 1)]?.focus(); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); links[Math.max(0, i - 1)]?.focus(); }
    else if (e.key === 'Home') { e.preventDefault(); links[0]?.focus(); }
    else if (e.key === 'End') { e.preventDefault(); links[links.length - 1]?.focus(); }
    else if (e.key === (rtl ? 'ArrowLeft' : 'ArrowRight') || e.key === (rtl ? 'ArrowRight' : 'ArrowLeft')) {
      const expand = e.key === (rtl ? 'ArrowLeft' : 'ArrowRight');
      const toggle = target.closest('[data-row]')?.querySelector<HTMLButtonElement>('[data-toggle]');
      if (toggle && (toggle.getAttribute('aria-expanded') === 'true') !== expand) { e.preventDefault(); toggle.click(); }
    }
  };

  const renderNode = (node: DeckTreeNode) => {
    const hasChildren = node.children.length > 0;
    const expanded = !node.deck.collapsed;
    const name = leaf(node.deck.name);
    const childId = `deck-children-${node.deck.id}`;
    return (
      <li key={node.deck.id} className={cx(s.item, hasChildren && s.parent)}>
        <div className={s.row} data-row style={{ '--level': node.level - 1 } as CSSProperties}>
          <div className={s.nameCell}>
            {hasChildren ? (
              <button
                type="button"
                className={s.toggle}
                data-toggle
                aria-expanded={expanded}
                aria-controls={childId}
                aria-label={t(expanded ? 'deck.collapse' : 'deck.expand', { name })}
                onClick={() => onToggle(node)}
              >
                <Icon name="chevron-down" size={16} />
              </button>
            ) : <span className={s.spacer} aria-hidden />}
            <a
              href={`#/study/${node.deck.id}`}
              className={s.link}
              data-deck-link
              title={stripHtml(node.deck.description).trim() || node.deck.name}
              aria-label={t('deck.rowLabel', { name, new: node.newCount, learn: node.learnCount, due: node.reviewCount })}
            >
              {name}
            </a>
          </div>
          <Count n={node.newCount} tone="new" label={t('counts.new', { count: node.newCount })} />
          <Count n={node.learnCount} tone="learn" label={t('counts.learn', { count: node.learnCount })} />
          <Count n={node.reviewCount} tone="due" label={t('counts.due', { count: node.reviewCount })} />
          <span className={s.time}>{node.estimatedMinutes > 0 ? t('time.short', { m: node.estimatedMinutes }) : ''}</span>
          <div className={s.actions}>
            <Button size="sm" variant="ghost" className={s.study} onClick={() => navigate(`#/study/${node.deck.id}`)} aria-label={t('deck.studyNamed', { name })}>
              {t('deck.study')}
            </Button>
            <Menu
              label={t('deck.menu', { name })}
              items={menuFor(node)}
              trigger={(p) => <IconButton {...p} size="sm" icon="more" label={t('deck.menu', { name })} />}
            />
          </div>
        </div>
        {hasChildren && (
          <ul role="list" id={childId} className={s.children} hidden={!expanded}>
            {node.children.map(renderNode)}
          </ul>
        )}
      </li>
    );
  };

  return (
    <section aria-labelledby="decks-title">
      <div className={s.sectionHead}>
        <h2 id="decks-title" className={s.sectionTitle}>{t('home.decks')}</h2>
      </div>
      <Card padding="none" className={s.tree}>
        <div className={s.head} aria-hidden>
          <span id="decks-title-col">{t('deck.col.deck')}</span>
          <span>{t('deck.col.new')}</span>
          <span>{t('deck.col.learn')}</span>
          <span>{t('deck.col.due')}</span>
          <span>{t('deck.col.time')}</span>
          <span />
        </div>
        <ul role="list" ref={listRef} className={s.list} onKeyDown={onKeyDown} aria-labelledby="decks-title">
          {root.children.map(renderNode)}
        </ul>
        <div className={s.footer}>
          <Button variant="ghost" icon="plus" onClick={() => setDialog({ kind: 'create' })}>{t('deck.new')}</Button>
          <span className={s.total}>{t('deck.totalCards', { n: formatCount(root.totalCards) })}</span>
        </div>
      </Card>
      <DeckDialogs state={dialog} onClose={() => setDialog(null)} />
      {exporting && (
        <ExportDialog
          open={exporting.open}
          onClose={() => setExporting((x) => (x ? { ...x, open: false } : x))}
          deckId={exporting.node.deck.id}
          deckName={exporting.node.deck.name}
        />
      )}
    </section>
  );
}
