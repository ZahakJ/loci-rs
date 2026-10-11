import { describe, expect, it } from 'vitest';
import { formatRoute, isFullBleed, navSection, parseRoute, type Route } from '@ui/routes';

describe('parseRoute', () => {
  const cases: Array<[string, Route]> = [
    ['', { name: 'home' }],
    ['#/', { name: 'home' }],
    ['#', { name: 'home' }],
    ['#/welcome', { name: 'welcome' }],
    ['#/study/all', { name: 'study', deckId: null }],
    ['#/study/1002', { name: 'study', deckId: 1002 }],
    ['#/study/all?custom=ahead%3A1', { name: 'study', deckId: null, custom: 'ahead:1' }],
    ['#/backlog/all', { name: 'backlog', deckId: null }],
    ['#/backlog/7', { name: 'backlog', deckId: 7 }],
    ['#/browse', { name: 'browse' }],
    ['#/browse?q=deck%3A%22A%3A%3AB%22%20is%3Adue', { name: 'browse', q: 'deck:"A::B" is:due' }],
    ['#/stats/all', { name: 'stats', deckId: null }],
    ['#/stats/3', { name: 'stats', deckId: 3 }],
    ['#/settings', { name: 'settings' }],
    ['#/settings?section=data', { name: 'settings', section: 'data' }],
    ['#/settings/presets/2', { name: 'preset', presetId: 2 }],
    ['#/settings/notetypes', { name: 'notetypes' }],
    ['#/import', { name: 'import' }],
    ['#/backups', { name: 'backups' }],
    ['#/note/1690000000000', { name: 'note', noteId: 1690000000000 }],
    ['#/add', { name: 'add' }],
    ['#/add?deck=5', { name: 'add', deckId: 5 }],
    ['/browse', { name: 'browse' }],
  ];
  it.each(cases)('%s', (hash, route) => expect(parseRoute(hash)).toEqual(route));

  it('rejects malformed paths', () => {
    for (const h of ['#/study/abc', '#/study/1/2', '#/note/x', '#/settings/presets/', '#/settings/presets/a', '#/nope', '#/import/x']) {
      expect(parseRoute(h).name).toBe('notFound');
    }
  });

  it('round-trips through formatRoute', () => {
    for (const [, route] of cases) {
      expect(parseRoute(formatRoute(route))).toEqual(route);
    }
  });
});

describe('route helpers', () => {
  it('maps routes to nav sections', () => {
    expect(navSection({ name: 'backlog', deckId: null })).toBe('home');
    expect(navSection({ name: 'note', noteId: 1 })).toBe('browse');
    expect(navSection({ name: 'preset', presetId: 1 })).toBe('settings');
    expect(navSection({ name: 'study', deckId: null })).toBeNull();
  });
  it('study and welcome are full-bleed', () => {
    expect(isFullBleed({ name: 'study', deckId: 1 })).toBe(true);
    expect(isFullBleed({ name: 'welcome' })).toBe(true);
    expect(isFullBleed({ name: 'home' })).toBe(false);
  });
});
