/** Remembers the Add sheet's last note type and deck (per browser; a convenience, never data). */
import { SAMPLE_DECK } from '@core/anki/sample-deck';

const KEY = 'loci.add.last';

export interface LastChoice { notetypeId?: number; deckId?: number; previewOpen?: boolean }

export function readLastChoice(): LastChoice {
  try {
    const raw = window.localStorage.getItem(KEY);
    const v = raw ? (JSON.parse(raw) as unknown) : null;
    return v && typeof v === 'object' ? (v as LastChoice) : {};
  } catch { return {}; }
}

export function writeLastChoice(patch: Partial<LastChoice>): void {
  try { window.localStorage.setItem(KEY, JSON.stringify({ ...readLastChoice(), ...patch })); } catch { /* storage unavailable */ }
}

/** True for the bundled sample deck (or a deck inside it). */
export function isSampleDeck(name: string, sampleName: string = SAMPLE_DECK.name): boolean {
  return name === sampleName || name.startsWith(`${sampleName}::`);
}

/**
 * The deck a new note goes to when the caller did not name one: the last deck used for adding;
 * otherwise a deck of the user's own ("Default" if it exists, else the first deck that is not the
 * bundled sample), so a newcomer's first notes do not land in the sample deck by accident; the
 * sample deck only when it is the only deck.
 */
export function defaultDeckId(decks: Array<{ id: number; name: string }>, lastDeckId: number | undefined, sampleName: string = SAMPLE_DECK.name): number | null {
  if (!decks.length) return null;
  const last = lastDeckId !== undefined ? decks.find((d) => d.id === lastDeckId) : undefined;
  if (last) return last.id;
  const own = decks.filter((d) => !isSampleDeck(d.name, sampleName));
  return (own.find((d) => d.name === 'Default') ?? own[0] ?? decks[0]!).id;
}
