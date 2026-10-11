import { describe, expect, it } from 'vitest';
import { SAMPLE_DECK } from '@core/anki/sample-deck';
import { defaultDeckId, isSampleDeck } from '@ui/views/edit/lastChoice';
import { exportFileName } from '@ui/views/edit/exportName';

const sample = { id: 10, name: SAMPLE_DECK.name };

describe('Add sheet default deck (N-F02)', () => {
  it('uses the last deck used for adding when it still exists', () => {
    expect(defaultDeckId([sample, { id: 1, name: 'Default' }, { id: 3, name: 'French' }], 3)).toBe(3);
    expect(defaultDeckId([sample, { id: 1, name: 'Default' }], 10)).toBe(10);
  });
  it('otherwise prefers Default, then the first deck that is not the sample', () => {
    expect(defaultDeckId([sample, { id: 3, name: 'French' }, { id: 1, name: 'Default' }], undefined)).toBe(1);
    expect(defaultDeckId([sample, { id: 3, name: 'French' }], 999)).toBe(3);
    expect(defaultDeckId([sample, { id: 11, name: `${SAMPLE_DECK.name}::Extra` }, { id: 4, name: 'Bio' }], undefined)).toBe(4);
  });
  it('falls back to the sample deck only when it is the only deck', () => {
    expect(defaultDeckId([sample], undefined)).toBe(10);
    expect(defaultDeckId([], undefined)).toBeNull();
  });
  it('recognises the sample deck and its children', () => {
    expect(isSampleDeck(SAMPLE_DECK.name)).toBe(true);
    expect(isSampleDeck(`${SAMPLE_DECK.name}::Sub`)).toBe(true);
    expect(isSampleDeck('Loci')).toBe(false);
  });
});

describe('deck export file name (V-F15)', () => {
  it('uses the last part of the deck name and drops unsafe characters', () => {
    expect(exportFileName('Languages::French')).toBe('French.apkg');
    expect(exportFileName('Loci: the art of memory')).toBe('Loci the art of memory.apkg');
    expect(exportFileName('a/b\\c?')).toBe('abc.apkg');
    expect(exportFileName('??')).toBe('deck.apkg');
    expect(exportFileName('العربية')).toBe('العربية.apkg');
  });
});
