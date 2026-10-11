import { useEffect, useState } from 'react';
import { t } from '@core/i18n/i18n';
import type { DeckTreeNode } from '@core/types';
import { api, useQuery } from '../../api';
import { Button, LinkButton } from '../../design/primitives/Button';
import { Card } from '../../design/primitives/Card';
import { EmptyState } from '../../design/primitives/EmptyState';
import { Skeleton } from '../../design/primitives/Skeleton';
import { toast } from '../../design/primitives/Toast';
import { Page } from '../../shell/Page';
import { navigate } from '../../routes';
import { CatchUpBanner } from './CatchUpBanner';
import { DeckTree } from './DeckTree';
import { InsightsStrip } from './InsightsStrip';
import { MomentumPanel } from './MomentumPanel';
import { TodayHero } from './TodayHero';
import s from './Home.module.css';
import { DeckDialogs, type DeckDialogState } from './DeckDialogs';
import { CustomSessionSheet } from '../study/CustomSessionSheet';
import { primaryPreset } from '../../hooks/presets';

function setCollapsed(node: DeckTreeNode, id: number, collapsed: boolean): DeckTreeNode {
  if (node.deck.id === id) return { ...node, deck: { ...node.deck, collapsed } };
  return { ...node, children: node.children.map((c) => setCollapsed(c, id, collapsed)) };
}

export default function HomeView() {
  const tree = useQuery((a) => a.deckTree(), [], ['decks', 'cards', 'presets', 'notes']);
  const backlog = useQuery((a) => a.backlogReport(null), [], ['decks', 'cards']);
  const insights = useQuery((a) => a.insights(), [], ['decks', 'cards', 'presets']);
  const stats = useQuery((a) => a.stats({ deckId: null }), [], ['cards']);
  const presets = useQuery((a) => a.listPresets(), [], ['presets']);
  const [customOpen, setCustomOpen] = useState(false);
  const budget = primaryPreset(tree.data, presets.data)?.config.minutesPerDay;
  const [dialog, setDialog] = useState<DeckDialogState>(null);

  useEffect(() => { document.title = 'Loci'; }, []);

  const onToggle = (node: DeckTreeNode) => {
    const next = !node.deck.collapsed;
    tree.setData((prev) => (prev ? setCollapsed(prev, node.deck.id, next) : prev!));
    api.setDeckCollapsed(node.deck.id, next).catch(() => toast({ message: t('common.saveFailed'), tone: 'danger' }));
  };

  const empty = tree.data && tree.data.children.length === 0;

  return (
    <Page title={t('nav.home')} docTitle="Loci" hideTitle width="wide">
      <div className={s.home}>
        {empty ? (
          <Card className={s.emptyCard}>
            <EmptyState
              title={t('home.empty.title')}
              body={t('home.empty.body')}
              actions={(
                <>
                  <Button variant="primary" icon="plus" onClick={() => setDialog({ kind: 'create' })} autoFocus>{t('deck.new')}</Button>
                  <LinkButton variant="secondary" icon="import" href="#/import">{t('home.empty.import')}</LinkButton>
                  <Button variant="ghost" icon="sparkle" onClick={async () => { await api.installSampleDeck(); toast({ message: t('welcome.sampleReady'), tone: 'success', icon: 'check' }); navigate('#/'); }}>{t('welcome.sample.title')}</Button>
                </>
              )}
            />
            <DeckDialogs state={dialog} onClose={() => setDialog(null)} />
          </Card>
        ) : (
          <>
            <TodayHero tree={tree.data} studiedSeconds={stats.data?.today.seconds} budgetMinutes={budget} onCustom={() => setCustomOpen(true)} />
            <CustomSessionSheet open={customOpen} onClose={() => setCustomOpen(false)} deckId={null} />
            {backlog.data?.triggered && <CatchUpBanner report={backlog.data} />}
            {insights.data && <InsightsStrip insights={insights.data} />}
            <div className={s.grid}>
              {tree.data ? <DeckTree root={tree.data} onToggle={onToggle} /> : <Skeleton height="22rem" radius="lg" />}
              <MomentumPanel stats={stats.data} />
            </div>
          </>
        )}
      </div>
    </Page>
  );
}
