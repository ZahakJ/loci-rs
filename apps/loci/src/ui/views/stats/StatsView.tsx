/**
 * Statistics that say what to do (DESIGN §12): scope picker, Insights, then panels that are each
 * a chart, one sentence of meaning and, where useful, one action.
 */
import { useMemo, useState, type ReactNode } from 'react';
import { getLocale, t } from '@core/i18n/i18n';
import type { DeckTreeNode, Stats } from '@core/types';
import { useQuery } from '@ui/api';
import { Bars, Histogram, Lines, Ring, Sparkline, StackedBar } from '@ui/charts';
import { Icon } from '@ui/design/icons';
import { Card } from '@ui/design/primitives/Card';
import { EmptyState } from '@ui/design/primitives/EmptyState';
import { Heatmap } from '@ui/design/primitives/Heatmap';
import { Segmented } from '@ui/design/primitives/Segmented';
import { Skeleton } from '@ui/design/primitives/Skeleton';
import { cx } from '@ui/design/primitives/layer';
import { formatCount, formatHour, formatMinutes, formatPercent } from '@ui/hooks/format';
import { navigate } from '@ui/routes';
import { Page } from '@ui/shell/Page';
import { InsightsStrip } from '../home/InsightsStrip';
import { Combobox } from '../edit/Combobox';
import { buttonShares, cardTotals, forecastSummary, hourlySummary, medianBucket, modeBucket, retentionHeadline, retentionRows, reviewSummary, rollingMean, shareFrom } from './summaries';
import s from './Stats.module.css';

function flatten(node: DeckTreeNode | undefined): Array<{ id: number; name: string }> {
  const out: Array<{ id: number; name: string }> = [];
  const walk = (n: DeckTreeNode) => { for (const c of n.children) { out.push({ id: c.deck.id, name: c.deck.name }); walk(c); } };
  if (node) walk(node);
  return out;
}

function Panel({ id, title, children, sentence, action, control, className }: { id: string; title: string; children: ReactNode; sentence?: ReactNode; action?: { label: string; href: string }; control?: ReactNode; className?: string }) {
  return (
    <Card as="section" padding="none" className={cx(s.panel, className)} aria-labelledby={`${id}-h`}>
      <header className={s.panelHead}>
        <h2 id={`${id}-h`} className={s.panelTitle}>{title}</h2>
        {control}
      </header>
      <div className={s.panelBody}>{children}</div>
      {(sentence || action) && (
        <footer className={s.panelFoot}>
          {sentence && <p className={s.sentence}>{sentence}</p>}
          {action && <a className={s.action} href={action.href}>{action.label}<Icon name="chevron-right" size={14} className="icon-flip" /></a>}
        </footer>
      )}
    </Card>
  );
}

const listFormat = (items: string[]) => {
  try { return new Intl.ListFormat(getLocale(), { style: 'long', type: 'conjunction' }).format(items); } catch { return items.join(', '); }
};
const dayFmt = () => new Intl.DateTimeFormat(getLocale(), { day: 'numeric', month: 'short' });
const monthFmt = () => new Intl.DateTimeFormat(getLocale(), { month: 'short' });

export default function StatsView({ deckId }: { deckId: number | null }) {
  const stats = useQuery((a) => a.stats({ deckId }), [deckId], ['cards', 'decks', 'notes']);
  const tree = useQuery((a) => a.deckTree(), [], ['decks']);
  const decks = useMemo(() => flatten(tree.data), [tree.data]);
  const [range, setRange] = useState<'30' | '90'>('30');
  const [rpd, setRpd] = useState<'reviews' | 'minutes'>('reviews');

  const scope = (
    <Combobox
      label={t('stats.scope')}
      hideLabel
      value={deckId ?? 0}
      options={[{ value: 0, label: t('stats.scopeCollection'), icon: 'layers' as const }, ...decks.map((d) => {
        const parts = d.name.split('::');
        const leaf = parts.pop()!;
        return { value: d.id, label: leaf, prefix: parts.length ? `${parts.join(' / ')} / ` : undefined, icon: 'deck' as const };
      })]}
      onChange={(v) => navigate({ name: 'stats', deckId: v === 0 ? null : v })}
      icon={deckId ? 'deck' : 'layers'}
      className={s.scope}
    />
  );

  const data = stats.data;
  const empty = data && data.reviewsPerDay.every((d) => d.count === 0) && cardTotals(data.cardCounts).total === 0;

  return (
    <Page title={t('stats.title')} width="wide" actions={scope} subtitle={t('stats.subtitle')}>
      {stats.error ? (
        <EmptyState icon="stats" title={t('stats.failed')} body={String((stats.error as Error)?.message ?? stats.error)} />
      ) : !data ? (
        <div className={s.grid} aria-busy="true" aria-label={t('common.loading')}>
          {Array.from({ length: 6 }, (_, i) => (
            <Card key={i} className={cx(s.panel, i === 0 ? s.full : s.half)}><Skeleton width="30%" height="1.25rem" /><Skeleton height="9rem" radius="md" /></Card>
          ))}
        </div>
      ) : empty ? (
        <EmptyState icon="stats" title={t('stats.noData')} body={t('stats.noDataBody')} />
      ) : (
        <div className={cx(s.stack, stats.fetching && s.refetching)}>
          {data.insights.length > 0 && <InsightsStrip insights={data.insights} />}
          <div className={s.grid}>
            <TodayPanel data={data} />
            <ForecastPanel data={data} range={range} setRange={setRange} />
            <RetentionPanel data={data} />
            <ReviewsPanel data={data} mode={rpd} setMode={setRpd} />
            <Panel id="activity" title={t('stats.heatmap')} className={s.full}
              sentence={t('stats.activitySentence', { days: data.heatmap.filter((d) => d.count > 0).length, total: data.heatmap.reduce((a, d) => a + d.count, 0) })}>
              <Heatmap data={data.heatmap} locale={getLocale()} countLabel={(count) => t('stats.reviewsCount', { count })} summaryLabel={(v) => t('stats.heatmapSummary', v)} lessLabel={t('common.less')} moreLabel={t('common.more')} />
            </Panel>
            <DistributionPanels data={data} />
            <HourlyPanel data={data} />
            <ButtonsPanel data={data} />
            <CountsPanel data={data} />
            <MomentumPanel data={data} />
          </div>
        </div>
      )}
    </Page>
  );
}

function TodayPanel({ data }: { data: Stats }) {
  const td = data.today;
  const minutes = formatMinutes(td.seconds / 60);
  return (
    <Panel id="today" title={t('stats.today')} className={s.full} sentence={t('stats.todaySentence', { count: td.studied, duration: minutes })}>
      <div className={s.today}>
        <Ring value={td.studied ? td.correctPct : 0} label={t('stats.correctToday', { pct: formatPercent(td.correctPct) })} center={td.studied ? formatPercent(td.correctPct) : '–'} tone="good" />
        <dl className={s.tiles}>
          <div className={s.tile}><dt>{t('stats.tile.studied')}</dt><dd>{formatCount(td.studied)}</dd></div>
          <div className={s.tile}><dt>{t('stats.tile.time')}</dt><dd>{minutes}</dd></div>
          <div className={s.tile}><dt>{t('stats.tile.again')}</dt><dd>{formatCount(td.again)}</dd></div>
          <div className={s.tile}><dt>{t('stats.tile.learned')}</dt><dd>{formatCount(td.learned)}</dd></div>
          <div className={s.tile}><dt>{t('stats.tile.relearned')}</dt><dd>{formatCount(td.relearned)}</dd></div>
        </dl>
      </div>
    </Panel>
  );
}

function ForecastPanel({ data, range, setRange }: { data: Stats; range: '30' | '90'; setRange: (r: '30' | '90') => void }) {
  const days = Number(range);
  const sum = forecastSummary(data.forecast, days);
  const fmt = dayFmt();
  const now = Date.now();
  const label = (d: number) => (d === 0 ? t('stats.todayShort') : fmt.format(now + d * 86_400_000));
  const rows = data.forecast.slice(0, days).map((d) => ({ label: label(d.day), values: [d.due, Math.max(0, d.withNew - d.due)] }));
  return (
    <Panel id="forecast" title={t('stats.forecast')} className={s.wideHalf}
      control={<Segmented label={t('stats.forecastRange')} value={range} onChange={setRange} options={[{ value: '30', label: t('stats.days', { count: 30 }) }, { value: '90', label: t('stats.days', { count: 90 }) }]} />}
      sentence={sum.peak > 0 ? t('stats.forecastSentence', { avg: sum.avg, withNew: sum.avgWithNew, peak: sum.peak, day: label(sum.peakDay) }) : t('stats.forecastNone', { days })}
      action={data.forecast[0] && data.forecast[0].due > sum.avg * 2 ? { label: t('stats.planCatchUp'), href: '#/backlog/all' } : undefined}>
      <Bars
        data={rows}
        series={[{ key: 'due', label: t('stats.forecastDue'), tone: 'due' }, { key: 'new', label: t('stats.forecastWithNew'), tone: 'new' }]}
        title={t('stats.forecast')}
        desc={t('stats.forecastDesc', { days })}
        categoryHeader={t('stats.day')}
        height={170}
        highlight={0}
      />
    </Panel>
  );
}

function RetentionPanel({ data }: { data: Stats }) {
  const head = retentionHeadline(data.retention);
  const lowGap = head && head.gap < -0.02;
  const { shown, hidden } = retentionRows(data.retention);
  return (
    <Panel id="retention" title={t('stats.retention')} className={s.narrowHalf}
      sentence={head ? (lowGap ? t('stats.retentionLow', { actual: formatPercent(head.actual), target: formatPercent(head.target), period: head.label }) : t('stats.retentionOk', { actual: formatPercent(head.actual), target: formatPercent(head.target), period: head.label })) : t('stats.noData')}
      action={lowGap ? { label: t('stats.optimise'), href: '#/settings' } : undefined}>
      {shown.length > 0 && (
      <ul role="list" className={s.bullets} aria-label={t('stats.retention')}>
        {shown.map((r) => {
          const v = r.trueRetention;
          const below = v !== null && v < r.desired - 0.005;
          return (
            <li key={r.label} className={s.bulletRow}>
              <span className={s.bulletLabel}>{r.label}</span>
              <span className={s.track} aria-hidden>
                {v !== null && <span className={cx(s.fill, below && s.fillLow)} style={{ inlineSize: `${v * 100}%` }} />}
                <span className={s.target} style={{ insetInlineStart: `${r.desired * 100}%` }} />
              </span>
              <span className={s.bulletValue}>
                {v === null ? '–' : formatPercent(v)}
                <span className="sr-only"> {t('stats.retentionRowSr', { target: formatPercent(r.desired), reviews: r.reviews })}</span>
              </span>
            </li>
          );
        })}
      </ul>
      )}
      {/* With no period worth drawing, the panel's sentence already says "Not enough reviews yet." */}
      {hidden.length > 0 && shown.length > 0 && (
        <p className={s.notEnough}>{t('stats.retentionHidden', { periods: listFormat(hidden.map((r) => r.label)) })}</p>
      )}
      {shown.length > 0 && <p className={s.legendNote} aria-hidden><span className={s.targetKey} />{t('stats.targetLine')}</p>}
    </Panel>
  );
}

function ReviewsPanel({ data, mode, setMode }: { data: Stats; mode: 'reviews' | 'minutes'; setMode: (m: 'reviews' | 'minutes') => void }) {
  const sum = reviewSummary(data.reviewsPerDay);
  const fmt = dayFmt();
  const mfmt = monthFmt();
  const crtToday = data.reviewsPerDay[data.reviewsPerDay.length - 1]?.day ?? 0;
  const dateOf = (day: number) => Date.now() - (crtToday - day) * 86_400_000;
  const first = new Set<number>();
  let lastMonth = -1;
  data.reviewsPerDay.forEach((d, i) => { const m = new Date(dateOf(d.day)).getMonth(); if (m !== lastMonth) { first.add(i); lastMonth = m; } });
  const minutes = rollingMean(data.reviewsPerDay.map((d) => d.seconds / 60), 7);
  return (
    <Panel id="reviews" title={t('stats.reviewsPerDay')} className={s.full}
      control={<Segmented label={t('stats.reviewsShow')} value={mode} onChange={setMode} options={[{ value: 'reviews', label: t('stats.reviews') }, { value: 'minutes', label: t('stats.minutes') }]} />}
      sentence={sum.studied ? t('stats.reviewsSentence', { reviews: sum.avgReviews, minutes: formatMinutes(sum.avgMinutes), days: sum.studied }) : t('stats.reviewsNone')}>
      {mode === 'reviews' ? (
        <Bars
          data={data.reviewsPerDay.map((d) => ({ label: first.has(data.reviewsPerDay.indexOf(d)) ? mfmt.format(dateOf(d.day)) : fmt.format(dateOf(d.day)), values: [d.review, d.learn, d.relearn] }))}
          series={[{ key: 'review', label: t('stats.review'), tone: 'due' }, { key: 'learn', label: t('stats.learning'), tone: 'learn' }, { key: 'relearn', label: t('stats.relearn'), tone: 'again' }]}
          title={t('stats.reviewsPerDay')}
          desc={t('stats.reviewsDesc')}
          categoryHeader={t('stats.day')}
          height={150}
          maxXLabels={12}
        />
      ) : (
        <Lines
          data={data.reviewsPerDay.map((d, i) => ({ label: fmt.format(dateOf(d.day)), values: [Math.round((minutes[i] ?? 0) * 10) / 10] }))}
          series={[{ key: 'min', label: t('stats.minutesAvg'), tone: 'accent', area: true }]}
          title={t('stats.minutesPerDay')}
          desc={t('stats.minutesDesc')}
          categoryHeader={t('stats.day')}
          format={(v) => formatMinutes(v)}
          yTick={(v) => `${v}`}
          height={150}
          maxXLabels={12}
        />
      )}
    </Panel>
  );
}

function DistributionPanels({ data }: { data: Stats }) {
  const ivl = medianBucket(data.intervals);
  const stab = modeBucket(data.stability);
  const hardShare = shareFrom(data.difficulty, 7);
  return (
    <>
      <Panel id="intervals" title={t('stats.intervals')} className={s.third} sentence={ivl ? t('stats.intervalsSentence', { bucket: ivl }) : t('stats.noData')}>
        <Histogram buckets={data.intervals} title={t('stats.intervals')} desc={t('stats.intervalsDesc')} seriesLabel={t('stats.cards')} categoryHeader={t('stats.interval')} />
      </Panel>
      <Panel id="stability" title={t('stats.stability')} className={s.third} sentence={stab ? t('stats.stabilitySentence', { bucket: stab.bucket, share: formatPercent(stab.share) }) : t('stats.noData')}>
        <Histogram buckets={data.stability} title={t('stats.stability')} desc={t('stats.stabilityDesc')} seriesLabel={t('stats.cards')} categoryHeader={t('stats.stability')} tone="easy" />
      </Panel>
      <Panel id="difficulty" title={t('stats.difficulty')} className={s.third}
        sentence={t('stats.difficultySentence', { share: formatPercent(hardShare) })}
        action={hardShare > 0.1 ? { label: t('stats.seeHardCards'), href: `#/browse?q=${encodeURIComponent('prop:d>0.7')}` } : undefined}>
        <Histogram buckets={data.difficulty.map((b) => ({ ...b, bucket: b.bucket.replace(/–\d+%$/, '%').replace(/^(\d+)%$/, '$1') }))} title={t('stats.difficulty')} desc={t('stats.difficultyDesc')} seriesLabel={t('stats.cards')} categoryHeader={t('stats.difficulty')} tone="hard" />
      </Panel>
    </>
  );
}

function HourlyPanel({ data }: { data: Stats }) {
  const h = hourlySummary(data.hourly);
  return (
    <Panel id="hourly" title={t('stats.hourly')} className={s.half}
      sentence={h ? t('stats.hourlySentence', { best: formatHour(h.best), pct: formatPercent(h.bestPct), busiest: formatHour(h.busiest) }) : t('stats.noData')}>
      <Bars
        data={data.hourly.map((x) => ({ label: formatHour(x.hour), values: [x.reviews], extra: [{ label: t('stats.correct'), value: formatPercent(x.correctPct) }] }))}
        series={[{ key: 'reviews', label: t('stats.reviews'), tone: 'accent' }]}
        title={t('stats.hourly')}
        desc={t('stats.hourlyDesc')}
        categoryHeader={t('stats.hour')}
        height={150}
        maxXLabels={6}
        highlight={h?.best}
      />
    </Panel>
  );
}

function ButtonsPanel({ data }: { data: Stats }) {
  const sh = buttonShares(data.buttons);
  const groups = [['learn', t('stats.learning')], ['young', t('stats.young')], ['mature', t('stats.mature')]] as const;
  return (
    <Panel id="buttons" title={t('stats.buttons')} className={s.half}
      sentence={t('stats.buttonsSentence', { correct: formatPercent(sh.mature.correct), hard: formatPercent(sh.mature.shares[1] ?? 0) })}>
      <Bars
        layout="grouped"
        data={groups.map(([k, label]) => ({ label, values: sh[k].shares, extra: [{ label: t('stats.answers'), value: formatCount(sh[k].total) }] }))}
        series={[{ key: 'again', label: t('study.again'), tone: 'again' }, { key: 'hard', label: t('study.hard'), tone: 'hard' }, { key: 'good', label: t('study.good'), tone: 'good' }, { key: 'easy', label: t('study.easy'), tone: 'easy' }]}
        title={t('stats.buttons')}
        desc={t('stats.buttonsDesc')}
        categoryHeader={t('stats.group')}
        format={(v) => formatPercent(v)}
        yTick={(v) => formatPercent(v)}
        yMax={1}
        yMin={0}
        height={150}
      />
    </Panel>
  );
}

function CountsPanel({ data }: { data: Stats }) {
  const c = data.cardCounts;
  const tot = cardTotals(c);
  return (
    <Panel id="counts" title={t('stats.cardCounts')} className={s.half}
      sentence={t('stats.countsSentence', { total: tot.total, mature: formatPercent(tot.matureShare) })}>
      <p className={s.bigFigure}>{formatCount(tot.total)}<span>{t('stats.cardsTotal')}</span></p>
      <StackedBar
        segments={[
          { key: 'new', label: t('stats.new'), value: c.new, tone: 'new' },
          { key: 'learning', label: t('stats.learning'), value: c.learning, tone: 'learn' },
          { key: 'young', label: t('stats.young'), value: c.young, tone: 'young' },
          { key: 'mature', label: t('stats.mature'), value: c.mature, tone: 'mature' },
          { key: 'suspended', label: t('stats.suspended'), value: c.suspended, tone: 'muted' },
          { key: 'buried', label: t('stats.buried'), value: c.buried, tone: 'faint' },
        ]}
        title={t('stats.cardCounts')}
        desc={t('stats.countsDesc')}
        categoryHeader={t('stats.state')}
        valueHeader={t('stats.cards')}
        format={formatCount}
        percent={(x) => formatPercent(x)}
      />
    </Panel>
  );
}

function MomentumPanel({ data }: { data: Stats }) {
  const m = data.momentum;
  const last30 = data.reviewsPerDay.slice(-30).map((d) => d.count);
  return (
    <Panel id="momentum" title={t('stats.momentum')} className={s.half} sentence={t('stats.momentumSentence', { days: m.daysStudiedLast30 })}>
      <div className={s.momentum}>
        <p className={s.bigFigure}>{m.daysStudiedLast30}<span>{t('stats.ofLast30')}</span></p>
        <dl className={s.runs}>
          <div><dt>{t('stats.currentRunLabel')}</dt><dd>{t('stats.daysCount', { count: m.currentRun })}</dd></div>
          <div><dt>{t('stats.longestRunLabel')}</dt><dd>{t('stats.daysCount', { count: m.longestRun })}</dd></div>
        </dl>
      </div>
      <Sparkline values={last30} fluid height={48} label={t('stats.last30Spark', { total: formatCount(last30.reduce((a, b) => a + b, 0)) })} />
    </Panel>
  );
}
