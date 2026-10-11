/**
 * The compact facts line in the Browse editor header: deck · due · interval · ease or difficulty
 * · reviews · lapses (pure; tested). Strings go through t().
 */
import { t } from '@core/i18n/i18n';
import { formatInterval } from '@core/scheduler/convert';
import { CardQueue, CardType, type Card, type CardExplanation } from '@core/types';
import { formatRelative } from '@ui/hooks/format';

export function cardMetaParts(card: Card, info: Pick<CardExplanation, 'dueDate' | 'difficulty'> | null, deckName: string | undefined, nowSecs: number = Date.now() / 1000): string[] {
  const out: string[] = [];
  if (deckName) out.push(deckName.split('::').join(' / '));
  if (card.queue === CardQueue.Suspended) out.push(t('browse.meta.suspended'));
  else if (card.queue === CardQueue.UserBuried || card.queue === CardQueue.SchedBuried) out.push(t('browse.meta.buried'));
  if (card.type === CardType.New) {
    out.push(t('browse.meta.new', { pos: card.due }));
  } else {
    const due = info?.dueDate ?? null;
    if (due !== null) out.push(t('browse.meta.due', { when: formatRelative(due, nowSecs) }));
    if (card.ivl > 0) out.push(t('browse.meta.interval', { value: formatInterval(card.ivl * 86_400) }));
    const d = info?.difficulty ?? (typeof card.data.d === 'number' ? card.data.d : null);
    if (d !== null) out.push(t('browse.meta.difficulty', { value: `${Math.round(((d - 1) / 9) * 100)}%` }));
    else if (card.factor > 0) out.push(t('browse.meta.ease', { value: `${Math.round(card.factor / 10)}%` }));
  }
  out.push(t('browse.meta.reviews', { count: card.reps }));
  if (card.type !== CardType.New) out.push(t('browse.meta.lapses', { count: card.lapses }));
  return out;
}
