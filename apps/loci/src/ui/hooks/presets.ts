/** Which preset matters most: decks → presets by card count (pure; tested). */
import type { DeckConfigPreset, DeckTreeNode } from '@core/types';

/** Cards per preset id, counting each deck's own cards once (a node's totalCards includes its children). */
export function cardsPerPreset(tree: DeckTreeNode | undefined): Map<number, number> {
  const out = new Map<number, number>();
  if (!tree) return out;
  const walk = (n: DeckTreeNode) => {
    if (n.level > 0) {
      const own = Math.max(0, n.totalCards - n.children.reduce((a, c) => a + c.totalCards, 0));
      out.set(n.deck.configId, (out.get(n.deck.configId) ?? 0) + own);
    }
    n.children.forEach(walk);
  };
  walk(tree);
  return out;
}

/** Decks (full names) that use a preset. */
export function decksUsing(tree: DeckTreeNode | undefined, presetId: number): string[] {
  if (!tree) return [];
  const out: string[] = [];
  const walk = (n: DeckTreeNode) => { if (n.level > 0 && n.deck.configId === presetId) out.push(n.deck.name); n.children.forEach(walk); };
  walk(tree);
  return out;
}

/**
 * The preset most of the collection studies with: most cards, then most decks, then the lowest id.
 * Undefined while either list is loading or there are no presets.
 */
export function primaryPreset(tree: DeckTreeNode | undefined, presets: DeckConfigPreset[] | undefined): DeckConfigPreset | undefined {
  if (!presets || presets.length === 0) return undefined;
  if (!tree) return undefined;
  const cards = cardsPerPreset(tree);
  const score = (p: DeckConfigPreset) => [cards.get(p.id) ?? 0, decksUsing(tree, p.id).length, -p.id] as const;
  return [...presets].sort((a, b) => {
    const x = score(a), y = score(b);
    return y[0] - x[0] || y[1] - x[1] || y[2] - x[2];
  })[0];
}
