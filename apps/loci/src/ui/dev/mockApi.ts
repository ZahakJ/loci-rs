/**
 * In-memory CollectionApi with realistic, seeded data (dev + screenshots). Selected by `?mock=1`
 * or VITE_MOCK_API=1. `?mock=1&empty=1` starts with an empty collection (first-run screen).
 * Implements every method of CollectionApi; writes emit 'changed' events like the worker does.
 */
import { LociError, type CollectionApi, type CollectionEvent } from '@core/api';
import * as T from '@core/types';
import { CardQueue, CardType, Rating, RevlogType } from '@core/types';
import type { ApiHandle, EventType } from '../api';
import {
  CRT_S, DECKS, NOTETYPES, NOTE_SEEDS, NOW_MS, PRESET_SEEDS, SAVED_SEARCHES, STANDARD_CONFIG, TODAY, mulberry32,
  type DeckSeed,
} from './mockData';

type What = Extract<CollectionEvent, { type: 'changed' }>['what'][number];
type Kind = 'new' | 'learn' | 'review';
interface QueueItem { cardId: number; kind: Kind; deckId: number }
interface UndoEntry { label: string; revert: () => void }

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));
const clone = <V,>(v: V): V => structuredClone(v);
const NOW_S = Math.floor(NOW_MS / 1000);
const DAY_S = 86400;

export function stripHtml(html: string): string {
  return html
    .replace(/\{\{c\d+::(.*?)(::.*?)?\}\}/g, '$1')
    .replace(/<[^>]*>/g, ' ')
    .replace(/&nbsp;/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();
}

function fmtInterval(seconds: number): string {
  if (seconds < 60) return '<1m';
  if (seconds < 3600) return `<${Math.max(1, Math.round(seconds / 60))}m`;
  if (seconds < DAY_S) return `${Math.round(seconds / 3600)}h`;
  const d = seconds / DAY_S;
  if (d < 30) return `${Math.round(d)}d`;
  if (d < 365) return `${(d / 30).toFixed(1).replace(/\.0$/, '')}mo`;
  return `${(d / 365).toFixed(1).replace(/\.0$/, '')}y`;
}

/** Relative workload of desired retention r versus 0.9 (FSRS-6 default decay). */
export function workloadFactor(r: number): number {
  const f = (x: number) => 1 / (Math.pow(x, -1 / 0.5) - 1);
  return f(r) / f(0.9);
}

class Emitter {
  private subs = new Map<EventType, Set<(e: CollectionEvent) => void>>();
  on(type: EventType, cb: (e: CollectionEvent) => void): () => void {
    let set = this.subs.get(type);
    if (!set) this.subs.set(type, (set = new Set()));
    set.add(cb);
    return () => set!.delete(cb);
  }
  emit(e: CollectionEvent) {
    for (const key of [e.type, '*'] as EventType[]) this.subs.get(key)?.forEach((cb) => { try { cb(e); } catch (err) { console.error(err); } });
  }
}

class MockCollection implements CollectionApi {
  private rand = mulberry32(20261006);
  private settings: T.CollectionSettings = {
    rolloverHour: 4, learnAheadMinutes: 20, locale: 'en', theme: 'system', cardFont: 'serif', cardFontSize: 22,
    cardAlign: 'center', showKeyboardHints: true, reducedMotion: 'system', typedAnswerIgnoreCase: true, typedAnswerIgnoreDiacritics: false,
  };
  private presets = new Map<number, T.DeckConfigPreset>();
  private decks = new Map<number, T.Deck>();
  private deckCounts = new Map<number, { new: number; learn: number; review: number; total: number }>();
  private notetypes = new Map<number, T.Notetype>();
  private notes = new Map<number, T.Note>();
  private cards = new Map<number, T.Card>();
  private revlog: T.RevlogEntry[] = [];
  private media = new Map<string, Uint8Array>();
  private saved = [...SAVED_SEARCHES];
  private backups: T.BackupInfo[] = [];
  private undoStack: UndoEntry[] = [];
  private queue: QueueItem[] = [];
  private session = { deckId: null as number | null, cards: 0, ms: 0, ratings: [0, 0, 0, 0], active: false };
  private nextId = NOW_MS;

  constructor(private events: Emitter, empty: boolean) {
    for (const p of PRESET_SEEDS) this.presets.set(p.id, { ...clone(p), mtime: CRT_S + 3600, usn: -1 });
    for (const nt of NOTETYPES) this.notetypes.set(nt.id, clone(nt));
    this.seedBackups();
    if (!empty) this.seedCollection();
  }

  // ───────── seeding ─────────
  private seedCollection() {
    for (const d of DECKS) this.addDeckRecord(d);
    let i = 0;
    for (const seed of NOTE_SEEDS) {
      const nid = 1_690_000_000_000 + i * 86_400_000;
      const note: T.Note = { id: nid, guid: `mock${i.toString(36).padStart(6, '0')}`, ntid: seed.ntid, mtime: CRT_S + i * 50_000, usn: -1, tags: [...seed.tags], fields: [...seed.fields] };
      this.notes.set(nid, note);
      const ords = this.cardOrds(note);
      for (const ord of ords) this.makeCard(note, seed.deck, ord, i);
      i++;
    }
    // review history for a few cards (explainCard)
    for (const c of Array.from(this.cards.values()).filter((c) => c.reps > 0)) {
      for (let k = 0; k < Math.min(c.reps, 6); k++) {
        const ago = (c.reps - k) * 9 * DAY_S;
        this.revlog.push({ id: (NOW_S - ago) * 1000 + k, cid: c.id, usn: -1, ease: k === 1 && c.lapses ? 1 : 3, ivl: Math.max(1, Math.round(c.ivl * (k + 1) / c.reps)), lastIvl: 0, factor: 2500, time: 4000 + Math.round(this.rand() * 9000), type: k === 0 ? RevlogType.Learn : RevlogType.Review });
      }
    }
  }

  private addDeckRecord(d: DeckSeed) {
    this.decks.set(d.id, { id: d.id, name: d.name, configId: d.configId, description: d.description, collapsed: false, today: { day: TODAY, new: 0, rev: 0, lrn: 0, timeMs: 0 }, mtime: CRT_S, usn: -1 });
    this.deckCounts.set(d.id, { ...d.counts, total: d.totalCards });
  }

  private cardOrds(note: T.Note): number[] {
    const nt = this.notetypes.get(note.ntid)!;
    if (nt.kind === T.NotetypeKind.Cloze) {
      const ords = new Set<number>();
      for (const f of note.fields) for (const mm of f.matchAll(/\{\{c(\d+)::/g)) ords.add(Number(mm[1]) - 1);
      return ords.size ? [...ords].sort((a, b) => a - b) : [0];
    }
    return nt.templates.map((t) => t.ord);
  }

  private makeCard(note: T.Note, did: number, ord: number, i: number): T.Card {
    const r = this.rand();
    const state: Kind = r < 0.28 ? 'new' : r < 0.36 ? 'learn' : 'review';
    const ivl = state === 'review' ? Math.max(1, Math.round(Math.pow(this.rand(), 2) * 240)) : 0;
    const reps = state === 'new' ? 0 : 1 + Math.round(this.rand() * 14);
    const lapses = note.tags.includes('leech') ? 9 : state === 'review' ? Math.round(this.rand() * this.rand() * 4) : 0;
    const card: T.Card = {
      id: note.id + ord + 1, nid: note.id, did, ord, mtime: note.mtime, usn: -1,
      type: state === 'new' ? CardType.New : state === 'learn' ? CardType.Learn : CardType.Review,
      queue: state === 'new' ? CardQueue.New : state === 'learn' ? CardQueue.Learn : CardQueue.Review,
      due: state === 'new' ? i : state === 'learn' ? NOW_S + 600 : TODAY + Math.round(this.rand() * 30) - 6,
      ivl, factor: state === 'review' ? 2500 : 0, reps, lapses, left: state === 'learn' ? 1 : 0, odue: 0, odid: 0,
      flags: i === 6 ? 1 : i === 25 ? 4 : 0,
      data: state === 'new' ? { pos: i } : { s: Math.max(0.5, ivl * 1.1 + this.rand() * 3), d: 2 + this.rand() * 7, dr: 0.9, decay: 0.1542 },
    };
    if (i === 38) card.queue = CardQueue.Suspended;
    this.cards.set(card.id, card);
    return card;
  }

  private seedBackups() {
    const at = (days: number, h = 0) => NOW_S - days * DAY_S - h * 3600;
    const list: Array<[T.BackupInfo['kind'], number, number]> = [
      ['daily', 0, 2], ['daily', 1, 5], ['daily', 2, 3], ['manual', 3, 7], ['daily', 4, 1], ['weekly', 9, 0], ['weekly', 16, 0], ['monthly', 36, 0], ['pre-restore', 52, 4], ['monthly', 67, 0],
    ];
    this.backups = list.map(([kind, d, h], k) => ({
      name: `loci-${new Date((at(d, h)) * 1000).toISOString().slice(0, 19).replace(/[:T]/g, '-')}-${kind}.sqlite`,
      at: at(d, h), bytes: 9_400_000 - k * 210_000 + Math.round(this.rand() * 90_000), cards: 4368 - k * 37, kind,
    }));
  }

  // ───────── helpers ─────────
  private async wait(ms = 90) { await sleep(ms); }
  private changed(...what: What[]) { this.events.emit({ type: 'changed', what }); }
  private progress(op: string, p: T.Progress, cb?: (p: T.Progress) => void) {
    cb?.(p);
    this.events.emit({ type: 'progress', op, progress: p });
  }
  private pushUndo(label: string, revert: () => void) { this.undoStack.push({ label, revert }); if (this.undoStack.length > 30) this.undoStack.shift(); }
  private snapshot() {
    return {
      decks: clone([...this.decks]), counts: clone([...this.deckCounts]), notes: clone([...this.notes]), cards: clone([...this.cards]), queue: clone(this.queue),
    };
  }
  private restore(s: ReturnType<MockCollection['snapshot']>) {
    this.decks = new Map(s.decks); this.deckCounts = new Map(s.counts); this.notes = new Map(s.notes); this.cards = new Map(s.cards); this.queue = s.queue;
  }
  private subtree(id: number | null): number[] {
    if (id === null) return [...this.decks.keys()];
    const root = this.decks.get(id);
    if (!root) return [];
    return [...this.decks.values()].filter((d) => d.id === id || d.name.startsWith(root.name + '::')).map((d) => d.id);
  }
  private deckName(id: number) { return this.decks.get(id)?.name ?? 'Default'; }
  private ensureDeckPath(name: string, configId = 1): T.Deck {
    const parts = name.split('::').map((p) => p.trim()).filter(Boolean);
    let path = '';
    let deck: T.Deck | undefined;
    for (const part of parts) {
      path = path ? `${path}::${part}` : part;
      deck = [...this.decks.values()].find((d) => d.name.toLowerCase() === path.toLowerCase());
      if (!deck) {
        const id = ++this.nextId;
        deck = { id, name: path, configId, description: '', collapsed: false, today: { day: TODAY, new: 0, rev: 0, lrn: 0, timeMs: 0 }, mtime: NOW_S, usn: -1 };
        this.decks.set(id, deck);
        this.deckCounts.set(id, { new: 0, learn: 0, review: 0, total: 0 });
      }
    }
    if (!deck) throw new LociError('invalid', 'A deck needs a name.');
    return deck;
  }
  private secsPerCard(kind: Kind) { return kind === 'review' ? 6 : kind === 'new' ? 18 : 10; }

  // ───────── lifecycle ─────────
  async open(): Promise<T.IntegrityReport> { await this.wait(120); return { ok: true, problems: [], repaired: [] }; }
  async close() { await this.wait(20); }
  async isEmpty() { await this.wait(30); return this.decks.size === 0 && this.notes.size === 0; }
  async getSettings() { await this.wait(30); return clone(this.settings); }
  async setSettings(patch: Partial<T.CollectionSettings>) { await this.wait(40); this.settings = { ...this.settings, ...patch }; this.changed('settings'); return clone(this.settings); }
  async today() { return TODAY; }

  // ───────── decks & presets ─────────
  async deckTree(): Promise<T.DeckTreeNode> {
    await this.wait(110);
    const all = [...this.decks.values()].sort((a, b) => a.name.localeCompare(b.name));
    const build = (deck: T.Deck, level: number): T.DeckTreeNode => {
      const prefix = deck.name + '::';
      const children = all.filter((d) => d.name.startsWith(prefix) && !d.name.slice(prefix.length).includes('::')).map((d) => build(d, level + 1));
      const own = this.deckCounts.get(deck.id) ?? { new: 0, learn: 0, review: 0, total: 0 };
      const sum = (k: 'new' | 'learn' | 'review' | 'total') => own[k] + children.reduce((a, c) => a + (k === 'new' ? c.newCount : k === 'learn' ? c.learnCount : k === 'review' ? c.reviewCount : c.totalCards), 0);
      const today = deck.today.day === TODAY ? deck.today.new : 0;
      const newCap = Math.max(0, (this.presets.get(deck.configId)?.config.newPerDay ?? 20) - today);
      const newCount = Math.min(sum('new'), newCap);
      const learnCount = sum('learn'), reviewCount = sum('review');
      return {
        deck: clone(deck), children, level, newCount, learnCount, reviewCount, totalCards: sum('total'),
        estimatedMinutes: Math.round((newCount * 18 + learnCount * 10 + reviewCount * 6) / 60),
      };
    };
    const tops = all.filter((d) => !d.name.includes('::')).map((d) => build(d, 1));
    const rootDeck: T.Deck = { id: 0, name: '', configId: 1, description: '', collapsed: false, today: { day: TODAY, new: 0, rev: 0, lrn: 0, timeMs: 0 }, mtime: NOW_S, usn: -1 };
    const s = (f: (n: T.DeckTreeNode) => number) => tops.reduce((a, n) => a + f(n), 0);
    return {
      deck: rootDeck, children: tops, level: 0, newCount: s((n) => n.newCount), learnCount: s((n) => n.learnCount), reviewCount: s((n) => n.reviewCount),
      totalCards: s((n) => n.totalCards), estimatedMinutes: s((n) => n.estimatedMinutes),
    };
  }
  async getDeck(id: number) { await this.wait(30); const d = this.decks.get(id); if (!d) throw new LociError('not-found', 'Deck not found.'); return clone(d); }
  async createDeck(name: string, configId?: number) {
    await this.wait(80);
    if (!name.trim()) throw new LociError('invalid', 'A deck needs a name.');
    if ([...this.decks.values()].some((d) => d.name.toLowerCase() === name.trim().toLowerCase())) throw new LociError('conflict', `A deck called “${name.trim()}” already exists.`);
    const d = this.ensureDeckPath(name.trim(), configId);
    this.changed('decks');
    return clone(d);
  }
  async renameDeck(id: number, name: string) {
    await this.wait(60);
    const d = this.decks.get(id);
    if (!d) throw new LociError('not-found', 'Deck not found.');
    const old = d.name;
    for (const x of this.decks.values()) if (x.name === old || x.name.startsWith(old + '::')) x.name = name + x.name.slice(old.length);
    this.changed('decks');
  }
  async deleteDeck(id: number) {
    await this.wait(120);
    const snap = this.snapshot();
    const ids = this.subtree(id);
    let cards = 0;
    for (const did of ids) { cards += this.deckCounts.get(did)?.total ?? 0; this.decks.delete(did); this.deckCounts.delete(did); }
    for (const c of [...this.cards.values()]) if (ids.includes(c.did)) this.cards.delete(c.id);
    this.pushUndo('Delete deck', () => this.restore(snap));
    this.changed('decks', 'cards');
    return { cards };
  }
  async extendTodayLimits(deckId: number, extraNew: number, extraReview: number): Promise<T.StudyCounts> {
    await this.wait(60);
    for (const id of this.subtree(deckId)) {
      const d = this.decks.get(id);
      if (!d) continue;
      if (d.today.day !== TODAY) d.today = { day: TODAY, new: 0, rev: 0, lrn: 0, timeMs: 0 };
      d.today.new -= Math.max(0, extraNew);
      d.today.rev -= Math.max(0, extraReview);
    }
    this.changed('decks');
    const find = (n: T.DeckTreeNode): T.DeckTreeNode | null => n.deck.id === deckId ? n : n.children.reduce<T.DeckTreeNode | null>((a, c) => a ?? find(c), null);
    const node = find(await this.deckTree());
    return { new: node?.newCount ?? 0, learn: node?.learnCount ?? 0, review: node?.reviewCount ?? 0 };
  }
  async setDeckCollapsed(id: number, collapsed: boolean) { await this.wait(20); const d = this.decks.get(id); if (d) d.collapsed = collapsed; this.changed('decks'); }
  async setDeckPreset(id: number, configId: number) { await this.wait(40); const d = this.decks.get(id); if (d) d.configId = configId; this.changed('decks', 'presets'); }
  async setDeckDescription(id: number, description: string) { await this.wait(40); const d = this.decks.get(id); if (d) d.description = description; this.changed('decks'); }
  async listPresets() { await this.wait(60); return clone([...this.presets.values()]); }
  async getPreset(id: number) { await this.wait(50); const p = this.presets.get(id); if (!p) throw new LociError('not-found', 'Preset not found.'); return clone(p); }
  async savePreset(p: Omit<T.DeckConfigPreset, 'mtime' | 'usn'> & { id?: number }) {
    await this.wait(90);
    const id = p.id && this.presets.has(p.id) ? p.id : Math.max(0, ...this.presets.keys()) + 1;
    const saved: T.DeckConfigPreset = { id, name: p.name, config: clone(p.config), mtime: NOW_S, usn: -1 };
    this.presets.set(id, saved);
    this.changed('presets');
    return clone(saved);
  }
  async deletePreset(id: number) {
    await this.wait(60);
    if (id === 1) throw new LociError('invalid', 'The default preset cannot be deleted.');
    this.presets.delete(id);
    for (const d of this.decks.values()) if (d.configId === id) d.configId = 1;
    this.changed('presets', 'decks');
  }
  async forecastWorkload(deckId: number | null, config: T.DeckConfig): Promise<T.WorkloadForecast> {
    await this.wait(140);
    const r = mulberry32(Math.round(config.desiredRetention * 1000) + config.newPerDay);
    const ids = this.subtree(deckId);
    const base = Math.max(8, ids.reduce((a, id) => a + (this.deckCounts.get(id)?.review ?? 0), 0) * 0.6);
    const wf = workloadFactor(config.desiredRetention);
    const reviewsPerDay = Array.from({ length: 90 }, (_, d) => {
      const steady = base * wf + config.newPerDay * 2.6 * (1 - Math.exp(-d / 25));
      return Math.max(0, Math.round(steady * (0.9 + r() * 0.2) * (d % 7 === 5 ? 1.08 : 1)));
    });
    const minutesPerDay = reviewsPerDay.map((n) => Math.round((n * 7 + config.newPerDay * 18) / 60));
    const avg = Math.round(reviewsPerDay.reduce((a, b) => a + b, 0) / 90);
    const mins = Math.round(minutesPerDay.reduce((a, b) => a + b, 0) / 90);
    return { reviewsPerDay, minutesPerDay, sentence: `About ${avg} reviews a day (${mins} min) over the next three months.` };
  }

  // ───────── note types ─────────
  async listNotetypes() { await this.wait(50); return clone([...this.notetypes.values()]); }
  async getNotetype(id: number) { await this.wait(30); const n = this.notetypes.get(id); if (!n) throw new LociError('not-found', 'Note type not found.'); return clone(n); }
  async saveNotetype(n: Omit<T.Notetype, 'mtime' | 'usn'> & { id?: number }) {
    await this.wait(120);
    const id = n.id && this.notetypes.has(n.id) ? n.id : ++this.nextId;
    const saved: T.Notetype = { ...clone(n), id, mtime: NOW_S, usn: -1 };
    this.notetypes.set(id, saved);
    this.changed('notetypes', 'cards');
    return clone(saved);
  }
  async deleteNotetype(id: number) {
    await this.wait(60);
    if ([...this.notes.values()].some((n) => n.ntid === id)) throw new LociError('conflict', 'Notes still use this note type. Delete or move them first.');
    this.notetypes.delete(id);
    this.changed('notetypes');
  }
  async previewNote(ntid: number, fields: string[], ord: number) {
    await this.wait(40);
    const note: T.Note = { id: 0, guid: '', ntid, mtime: NOW_S, usn: -1, tags: [], fields };
    return this.render(note, ord, 0);
  }
  async checkTemplate(ntid: number, qfmt: string, afmt: string) {
    await this.wait(30);
    const nt = this.notetypes.get(ntid);
    const names = new Set((nt?.fields ?? []).map((f) => f.name));
    const special = new Set(['FrontSide', 'Tags', 'Deck', 'Subdeck', 'Card', 'CardFlag', 'Type']);
    const out: string[] = [];
    for (const [label, src] of [['Front', qfmt], ['Back', afmt]] as const) {
      for (const mm of src.matchAll(/\{\{([#^/]?)([^}]+)\}\}/g)) {
        const name = mm[2]!.split(':').pop()!.trim();
        if (!names.has(name) && !special.has(name)) out.push(`${label} template: there is no field called “${name}”.`);
      }
      const opens = [...src.matchAll(/\{\{[#^]([^}]+)\}\}/g)].map((x) => x[1]);
      for (const o of opens) if (!src.includes(`{{/${o}}}`)) out.push(`${label} template: {{#${o}}} is never closed.`);
    }
    return out;
  }

  // ───────── notes & cards ─────────
  async getNote(id: number) { await this.wait(40); const n = this.notes.get(id); if (!n) throw new LociError('not-found', 'Note not found.'); return clone(n); }
  async getCard(id: number) { await this.wait(30); const c = this.cards.get(id); if (!c) throw new LociError('not-found', 'Card not found.'); return clone(c); }
  async getCardsOfNote(id: number) { await this.wait(30); return clone([...this.cards.values()].filter((c) => c.nid === id)); }
  async addNote(ntid: number, deckId: number, fields: string[], tags: string[]) {
    await this.wait(90);
    if (!stripHtml(fields[0] ?? '') && !(fields[0] ?? '').includes('<img')) throw new LociError('invalid', 'The first field is empty.');
    const id = ++this.nextId;
    const note: T.Note = { id, guid: `new${id.toString(36)}`, ntid, mtime: NOW_S, usn: -1, tags, fields };
    this.notes.set(id, note);
    const cardIds: number[] = [];
    for (const ord of this.cardOrds(note)) {
      const c = this.makeCard(note, deckId, ord, this.notes.size);
      c.type = CardType.New; c.queue = CardQueue.New; c.ivl = 0; c.reps = 0; c.lapses = 0; c.data = { pos: this.notes.size };
      cardIds.push(c.id);
    }
    const counts = this.deckCounts.get(deckId);
    if (counts) { counts.new += cardIds.length; counts.total += cardIds.length; }
    this.pushUndo('Add note', () => { this.notes.delete(id); cardIds.forEach((c) => this.cards.delete(c)); if (counts) { counts.new -= cardIds.length; counts.total -= cardIds.length; } });
    this.changed('notes', 'cards', 'decks');
    return { noteId: id, cardIds };
  }
  async updateNote(id: number, fields: string[], tags: string[]) {
    await this.wait(80);
    const n = this.notes.get(id);
    if (!n) throw new LociError('not-found', 'Note not found.');
    const before = clone(n);
    n.fields = fields; n.tags = tags; n.mtime = NOW_S;
    this.pushUndo('Edit note', () => this.notes.set(id, before));
    this.changed('notes');
    return { cardsAdded: 0, cardsRemoved: 0 };
  }
  async deleteNotes(ids: number[]) {
    await this.wait(80);
    const snap = this.snapshot();
    for (const id of ids) { this.notes.delete(id); for (const c of [...this.cards.values()]) if (c.nid === id) this.cards.delete(c.id); }
    this.pushUndo(ids.length === 1 ? 'Delete note' : `Delete ${ids.length} notes`, () => this.restore(snap));
    this.changed('notes', 'cards', 'decks');
  }
  async findDuplicates(ntid: number, first: string) {
    await this.wait(40);
    const key = stripHtml(first).toLowerCase();
    if (!key) return [];
    return [...this.notes.values()].filter((n) => n.ntid === ntid && stripHtml(n.fields[0] ?? '').toLowerCase() === key).map((n) => n.id);
  }
  async renderCard(id: number) {
    await this.wait(40);
    const c = this.cards.get(id);
    if (!c) throw new LociError('not-found', 'Card not found.');
    return this.render(this.notes.get(c.nid)!, c.ord, c.id);
  }
  async explainCard(id: number): Promise<T.CardExplanation> {
    await this.wait(60);
    const c = this.cards.get(id);
    if (!c) throw new LociError('not-found', 'Card not found.');
    const s = c.data.s ?? null;
    const elapsed = Math.max(1, c.ivl - Math.max(0, c.due - TODAY));
    const r = s ? Math.pow(1 + (19 / 81) * elapsed / s, -0.5) : null;
    const history = this.revlog.filter((e) => e.cid === id).sort((a, b) => b.id - a.id);
    return {
      retrievability: r, stability: s, difficulty: c.data.d ?? null, dueDate: c.type === CardType.Review ? CRT_S + c.due * DAY_S : null,
      interval: c.ivl, reps: c.reps, lapses: c.lapses, averageSeconds: 7.4, history,
      sentence: s ? `You will probably still remember this in ${Math.round(s)} days; it is scheduled for when recall drops to about 90%.` : 'You have not studied this card yet.',
    };
  }
  async allTags() {
    await this.wait(40);
    const m = new Map<string, number>();
    for (const n of this.notes.values()) for (const t of n.tags) m.set(t, (m.get(t) ?? 0) + 1);
    return [...m].sort((a, b) => a[0].localeCompare(b[0])).map(([tag, count]) => ({ tag, count }));
  }
  async renameTag(from: string, to: string) {
    await this.wait(60);
    let n = 0;
    const snap = this.snapshot();
    for (const note of this.notes.values()) { const i = note.tags.indexOf(from); if (i >= 0) { note.tags[i] = to; n++; } }
    this.pushUndo('Rename tag', () => this.restore(snap));
    this.changed('tags', 'notes');
    return n;
  }

  // ───────── media ─────────
  async addMedia(name: string, bytes: Uint8Array) {
    await this.wait(40);
    let clean = name.replace(/[\\/:*?"<>|]/g, '').trim() || 'pasted';
    if (this.media.has(clean)) { const dot = clean.lastIndexOf('.'); clean = `${dot > 0 ? clean.slice(0, dot) : clean}-${(++this.nextId).toString(36)}${dot > 0 ? clean.slice(dot) : ''}`; }
    this.media.set(clean, bytes);
    return clean;
  }
  async getMedia(name: string) { await this.wait(20); return this.media.get(name) ?? null; }
  async checkMedia() { await this.wait(700); return { unused: ['old-diagram.png', 'pasted-2023-04-02.jpg'], missing: ['heart-sounds.mp3'] }; }
  async deleteMedia(names: string[]) { await this.wait(60); names.forEach((n) => this.media.delete(n)); }

  // ───────── rendering ─────────
  private render(note: T.Note, ord: number, cardId: number): T.RenderedCard {
    const nt = this.notetypes.get(note.ntid) ?? NOTETYPES[0]!;
    const f = note.fields;
    const fieldDiv = (html: string) => `<div class="loci-field" dir="auto">${html}</div>`;
    const isRtl = /[؀-ۿ]/.test(stripHtml(f[0] ?? ''));
    const base = { cardId, noteId: note.id, notetypeId: nt.id, templateName: nt.templates[Math.min(ord, nt.templates.length - 1)]?.name ?? 'Card 1', css: '', dir: (isRtl ? 'auto' : 'auto') as T.RenderedCard['dir'] };
    const side = (html: string, extra: Partial<T.RenderedSide> = {}): T.RenderedSide => ({ html, media: [], hasTypeInput: false, ...extra });
    if (nt.config.builtin === 'image-occlusion') {
      const shapes: T.OcclusionShape[] = [...(f[0] ?? '').matchAll(/\{\{c(\d+)::image-occlusion:rect:left=([\d.]+):top=([\d.]+):width=([\d.]+):height=([\d.]+)/g)].map((mm) => ({
        kind: 'rect', ord: Number(mm[1]), left: Number(mm[2]), top: Number(mm[3]), width: Number(mm[4]), height: Number(mm[5]),
      }));
      const img = (f[1] ?? '').match(/src="([^"]+)"/)?.[1] ?? '';
      const header = f[2] ? `<div class="loci-io-header">${f[2]}</div>` : '';
      const html = `${header}<div class="loci-io"><img src="${img}" alt=""></div>`;
      return { ...base, front: side(html, { clozeOrds: [ord + 1] }), back: side(html + (f[3] ? `<div class="loci-extra">${f[3]}</div>` : ''), { clozeOrds: [ord + 1] }), occlusion: { image: img, shapes, mode: 'hide-all', activeOrd: ord + 1 } };
    }
    if (nt.kind === T.NotetypeKind.Cloze) {
      const active = ord + 1;
      const text = f[0] ?? '';
      const q = text.replace(/\{\{c(\d+)::(.*?)(?:::(.*?))?\}\}/g, (_m, n: string, body: string, hint?: string) =>
        Number(n) === active ? `<span class="cloze" data-ord="${n}">[${hint ?? '…'}]</span>` : body);
      const a = text.replace(/\{\{c(\d+)::(.*?)(?:::(.*?))?\}\}/g, (_m, n: string, body: string) =>
        Number(n) === active ? `<span class="cloze" data-ord="${n}">${body}</span>` : body);
      const extra = f[1] ? `<div class="loci-extra">${f[1]}</div>` : '';
      return { ...base, front: side(fieldDiv(q), { clozeOrds: [active] }), back: side(fieldDiv(a) + extra, { clozeOrds: [active] }) };
    }
    const [qf, af] = ord === 1 ? [f[1] ?? '', f[0] ?? ''] : [f[0] ?? '', f[1] ?? ''];
    if (nt.config.builtin === 'basic-typed') {
      return { ...base, front: side(fieldDiv(qf) + '<div class="loci-type-slot"></div>', { hasTypeInput: true, typeAnswer: stripHtml(af) }), back: side(fieldDiv(qf) + '<hr id="answer">' + '<div class="loci-type-slot"></div>', { hasTypeInput: true, typeAnswer: stripHtml(af) }) };
    }
    return { ...base, front: side(fieldDiv(qf)), back: side(fieldDiv(qf) + '<hr id="answer">' + fieldDiv(af)) };
  }

  // ───────── study ─────────
  private buildQueue(deckId: number | null) {
    const q: QueueItem[] = [];
    for (const did of this.subtree(deckId)) {
      const counts = this.deckCounts.get(did);
      if (!counts) continue;
      let pool = [...this.cards.values()].filter((c) => c.did === did && c.queue >= 0).map((c) => c.id);
      if (pool.length === 0) pool = [...this.cards.values()].filter((c) => c.queue >= 0).map((c) => c.id);
      if (pool.length === 0) continue;
      let k = 0;
      for (const kind of ['learn', 'review', 'new'] as Kind[]) for (let i = 0; i < counts[kind]; i++) q.push({ cardId: pool[k++ % pool.length]!, kind, deckId: did });
    }
    // interleave like Anki's "mix": learning first, then reviews and new spread out
    const learn = q.filter((x) => x.kind === 'learn'), rev = q.filter((x) => x.kind === 'review'), nw = q.filter((x) => x.kind === 'new');
    const mixed: QueueItem[] = [...learn];
    const step = nw.length ? Math.max(1, Math.floor(rev.length / nw.length)) : Infinity;
    let ni = 0;
    rev.forEach((r, i) => { mixed.push(r); if ((i + 1) % step === 0 && ni < nw.length) mixed.push(nw[ni++]!); });
    while (ni < nw.length) mixed.push(nw[ni++]!);
    this.queue = mixed;
  }
  private queueCounts(): T.StudyCounts {
    return { new: this.queue.filter((x) => x.kind === 'new').length, learn: this.queue.filter((x) => x.kind === 'learn').length, review: this.queue.filter((x) => x.kind === 'review').length };
  }
  private preview(kind: Kind, card: T.Card): T.SchedulingPreview {
    const secs: Record<Rating, number> = kind === 'new'
      ? { 1: 60, 2: 360, 3: 600, 4: 4 * DAY_S }
      : kind === 'learn'
        ? { 1: 60, 2: 330, 3: DAY_S, 4: 3 * DAY_S }
        : { 1: 600, 2: Math.max(1, Math.round(card.ivl * 1.2)) * DAY_S, 3: Math.max(2, Math.round(card.ivl * 2.4 + 1)) * DAY_S, 4: Math.max(4, Math.round(card.ivl * 3.6 + 2)) * DAY_S };
    return { secondsUntil: secs, labels: { 1: fmtInterval(secs[1]), 2: fmtInterval(secs[2]), 3: fmtInterval(secs[3]), 4: fmtInterval(secs[4]) } };
  }
  private current(): T.StudyCard | null {
    const item = this.queue[0];
    if (!item) return null;
    const card = this.cards.get(item.cardId)!;
    const note = this.notes.get(card.nid)!;
    return clone({
      card: { ...card, did: item.deckId }, note, rendered: this.render(note, card.ord, card.id), preview: this.preview(item.kind, card),
      counts: this.queueCounts(), deckName: this.deckName(item.deckId), isLeech: card.lapses >= 8,
    });
  }
  async startStudy(deckId: number | null) {
    await this.wait(140);
    this.session = { deckId, cards: 0, ms: 0, ratings: [0, 0, 0, 0], active: true };
    this.buildQueue(deckId);
    return this.current();
  }
  async nextCard() { await this.wait(40); return this.current(); }
  async answerCard(cardId: number, rating: Rating, elapsedMs: number): Promise<T.AnswerResult> {
    await this.wait(70);
    const item = this.queue[0];
    if (!item || item.cardId !== cardId) throw new LociError('conflict', 'That card is no longer current.');
    const snap = this.snapshot();
    const sess = clone(this.session);
    this.queue.shift();
    this.session.cards++; this.session.ms += Math.min(elapsedMs, 60_000); this.session.ratings[rating - 1]!++;
    const counts = this.deckCounts.get(item.deckId);
    if (counts) counts[item.kind] = Math.max(0, counts[item.kind] - 1);
    let becameLeech = false;
    if (rating === Rating.Again) {
      this.queue.splice(Math.min(3, this.queue.length), 0, { ...item, kind: 'learn' });
      if (counts) counts.learn++;
      const c = this.cards.get(cardId)!;
      if (item.kind === 'review') { c.lapses++; becameLeech = c.lapses === 8; }
    }
    const undoLabel = `Undo ${['Again', 'Hard', 'Good', 'Easy'][rating - 1]}`;
    this.pushUndo(undoLabel, () => { this.restore(snap); this.session = sess; });
    this.changed('cards', 'decks');
    const next = this.current();
    return { counts: this.queueCounts(), next, undoLabel, becameLeech: becameLeech || undefined, finished: next === null };
  }
  async startCustomStudy(deckId: number | null, kind: Parameters<CollectionApi['startCustomStudy']>[1]) {
    await this.wait(140);
    const ids = this.subtree(deckId);
    let pool = [...this.cards.values()].filter((c) => ids.includes(c.did) && c.queue >= 0);
    if (kind.kind === 'preview') pool = pool.filter((c) => c.type === CardType.New);
    if (kind.kind === 'forgotten') pool = pool.filter((c) => c.lapses > 0);
    if (kind.kind === 'search') pool = (await this.searchIds(kind.query, 'cards')).map((id) => this.cards.get(id)!).filter(Boolean).slice(0, kind.limit);
    this.session = { deckId, cards: 0, ms: 0, ratings: [0, 0, 0, 0], active: true };
    this.queue = pool.map((c) => ({ cardId: c.id, kind: c.type === CardType.New ? 'new' : 'review', deckId: c.did }));
    return this.current();
  }
  async endStudy(): Promise<T.SessionSummary> {
    await this.wait(80);
    const [again, hard, good, easy] = this.session.ratings as [number, number, number, number];
    const total = Math.max(1, again + hard + good + easy);
    this.session.active = false;
    return {
      cards: this.session.cards, seconds: Math.round(this.session.ms / 1000), accuracy: (total - again) / total, again, hard, good, easy,
      tomorrowDue: 38, insight: { id: 'pace', severity: 'info', text: 'Your answers were fastest in the first ten minutes.' },
    };
  }
  async suspendCards(ids: number[], suspend: boolean) {
    await this.wait(50);
    const snap = this.snapshot();
    for (const id of ids) { const c = this.cards.get(id); if (c) c.queue = suspend ? CardQueue.Suspended : (c.type as number as CardQueue); }
    if (suspend) this.queue = this.queue.filter((q) => !ids.includes(q.cardId));
    this.pushUndo(suspend ? 'Suspend' : 'Unsuspend', () => this.restore(snap));
    this.changed('cards', 'decks');
  }
  async buryCards(ids: number[]) {
    await this.wait(50);
    const snap = this.snapshot();
    for (const id of ids) { const c = this.cards.get(id); if (c) c.queue = CardQueue.UserBuried; }
    this.queue = this.queue.filter((q) => !ids.includes(q.cardId));
    this.pushUndo('Bury', () => this.restore(snap));
    this.changed('cards', 'decks');
  }
  async setFlag(ids: number[], flag: T.Flag) { await this.wait(30); for (const id of ids) { const c = this.cards.get(id); if (c) c.flags = flag; } this.changed('cards'); }
  async unburyDeck(deckId: number | null) {
    await this.wait(50);
    let n = 0;
    for (const c of this.cards.values()) if (this.subtree(deckId).includes(c.did) && (c.queue === CardQueue.UserBuried || c.queue === CardQueue.SchedBuried)) { c.queue = c.type as number as CardQueue; n++; }
    this.changed('cards');
    return n;
  }
  async resolveLeech(cardId: number, action: 'reset' | 'suspend' | 'keep') {
    await this.wait(50);
    const c = this.cards.get(cardId);
    if (!c) return;
    if (action === 'reset') { c.type = CardType.New; c.queue = CardQueue.New; c.lapses = 0; c.ivl = 0; }
    if (action === 'suspend') c.queue = CardQueue.Suspended;
    this.changed('cards');
  }

  // ───────── undo ─────────
  async undo() {
    await this.wait(60);
    const e = this.undoStack.pop();
    if (!e) return null;
    e.revert();
    this.changed('decks', 'notes', 'cards', 'tags');
    return { label: e.label };
  }
  async undoLabel() { return this.undoStack[this.undoStack.length - 1]?.label ?? null; }

  // ───────── backlog ─────────
  async backlogReport(deckId: number | null): Promise<T.BacklogReport> {
    await this.wait(80);
    const ids = this.subtree(deckId);
    const due = ids.reduce((a, id) => a + (this.deckCounts.get(id)?.review ?? 0), 0);
    const dailyAverage = 31;
    const overdue = Math.round(due * 0.55);
    const maxOverdueDays = due > 0 ? 6 : 0;
    return {
      due, overdue, maxOverdueDays, dailyAverage, estimatedMinutes: Math.round(due * 6 / 60),
      daysAtCurrentPace: Math.ceil(due / dailyAverage), triggered: due > 2 * dailyAverage || maxOverdueDays > 3,
    };
  }
  async applyCatchUp(deckId: number | null, plan: T.CatchUpPlan): Promise<T.CatchUpResult> {
    await this.wait(300);
    const snap = this.snapshot();
    const ids = this.subtree(deckId);
    let postponed = 0, kept = 0;
    for (const id of ids) {
      const c = this.deckCounts.get(id);
      if (!c) continue;
      const keep = plan.kind === 'study-all' ? c.review : plan.kind === 'spread' ? Math.ceil(c.review / plan.days) : plan.kind === 'today-share' ? Math.ceil(c.review / 3) : Math.ceil(c.review * 0.7);
      postponed += c.review - keep; kept += keep; c.review = keep;
    }
    this.pushUndo('Catch-up plan', () => this.restore(snap));
    this.changed('cards', 'decks');
    return { postponed, keptToday: kept, undoLabel: 'Catch-up plan' };
  }

  // ───────── browse ─────────
  private matches(c: T.Card, n: T.Note, query: string): boolean {
    const tokens = query.match(/-?"[^"]*"|-?[^\s"]+(?:"[^"]*")?/g) ?? [];
    const plain = n.fields.map(stripHtml).join(' ').toLowerCase();
    return tokens.every((raw) => {
      if (raw.toUpperCase() === 'OR' || raw === '(' || raw === ')') return true;
      const neg = raw.startsWith('-');
      const tok = (neg ? raw.slice(1) : raw).replace(/"/g, '');
      const [k, ...rest] = tok.split(':');
      const v = rest.join(':').toLowerCase();
      let ok: boolean;
      switch (rest.length ? k!.toLowerCase() : '') {
        case 'deck': { const name = this.deckName(c.did).toLowerCase(); ok = name === v || name.startsWith(v.replace(/\*$/, '')) ; break; }
        case 'tag': ok = n.tags.some((t) => t.toLowerCase() === v || (v.endsWith('*') && t.toLowerCase().startsWith(v.slice(0, -1)))); break;
        case 'note': ok = (this.notetypes.get(n.ntid)?.name.toLowerCase() ?? '') === v; break;
        case 'flag': ok = c.flags === Number(v); break;
        case 'is':
          ok = v === 'new' ? c.type === CardType.New : v === 'learn' ? c.type === CardType.Learn || c.type === CardType.Relearn : v === 'review' ? c.type === CardType.Review
            : v === 'due' ? (c.type === CardType.Review && c.due <= TODAY) || c.queue === CardQueue.Learn : v === 'suspended' ? c.queue === CardQueue.Suspended
              : v === 'buried' ? c.queue === CardQueue.UserBuried || c.queue === CardQueue.SchedBuried : true;
          break;
        case 'nid': ok = String(n.id) === v; break;
        case 'cid': ok = String(c.id) === v; break;
        default: ok = tok === '' || tok === '*' || plain.includes(tok.toLowerCase());
      }
      return neg ? !ok : ok;
    });
  }
  private row(c: T.Card, mode: T.BrowseMode): T.BrowseRow {
    const n = this.notes.get(c.nid)!;
    const nt = this.notetypes.get(n.ntid);
    const dueText = c.type === CardType.New ? `New #${c.data.pos ?? 0}` : c.type === CardType.Review ? new Date((CRT_S + c.due * DAY_S) * 1000).toISOString().slice(0, 10) : 'Today';
    const date = (s: number) => new Date(s * 1000).toISOString().slice(0, 10);
    const cells: T.BrowseRow['cells'] = {
      sortField: stripHtml(n.fields[0] ?? '').slice(0, 80), question: stripHtml(n.fields[0] ?? '').slice(0, 80), answer: stripHtml(n.fields[1] ?? '').slice(0, 80),
      deck: this.deckName(c.did), notetype: nt?.name ?? '', template: nt?.templates[c.ord]?.name ?? `Cloze ${c.ord + 1}`, tags: n.tags.join(' '),
      due: dueText, interval: c.ivl ? fmtInterval(c.ivl * DAY_S) : '', ease: c.factor ? `${c.factor / 10}%` : '',
      difficulty: c.data.d ? `${Math.round((c.data.d - 1) / 9 * 100)}%` : '', stability: c.data.s ? fmtInterval(c.data.s * DAY_S) : '',
      retrievability: c.data.s ? `${Math.round(Math.pow(1 + (19 / 81) * 3 / c.data.s, -0.5) * 100)}%` : '', reps: String(c.reps), lapses: String(c.lapses),
      created: date(Math.floor(n.id / 1000)), modified: date(n.mtime), cardCount: String([...this.cards.values()].filter((x) => x.nid === n.id).length),
    };
    return { id: mode === 'notes' ? n.id : c.id, cardId: c.id, noteId: n.id, cells, flag: (c.flags & 7) as T.Flag, suspended: c.queue === CardQueue.Suspended, buried: c.queue < -1, isLeech: n.tags.includes('leech') };
  }
  async browse(req: T.BrowseRequest): Promise<T.BrowseResult> {
    await this.wait(120);
    let cards = [...this.cards.values()].filter((c) => this.matches(c, this.notes.get(c.nid)!, req.query));
    if (req.mode === 'notes') { const seen = new Set<number>(); cards = cards.filter((c) => (seen.has(c.nid) ? false : (seen.add(c.nid), true))); }
    let rows = cards.map((c) => this.row(c, req.mode));
    const col = req.sort.column;
    rows.sort((a, b) => (a.cells[col] ?? '').localeCompare(b.cells[col] ?? '', undefined, { numeric: true }) * (req.sort.descending ? -1 : 1));
    const ids = rows.map((r) => r.id);
    rows = rows.slice(req.offset, req.offset + req.limit);
    return { rows, total: ids.length, ids };
  }
  async searchIds(query: string, mode: T.BrowseMode) {
    await this.wait(60);
    const cards = [...this.cards.values()].filter((c) => this.matches(c, this.notes.get(c.nid)!, query));
    return mode === 'notes' ? [...new Set(cards.map((c) => c.nid))] : cards.map((c) => c.id);
  }
  async sidebar(): Promise<T.SidebarItem[]> {
    await this.wait(80);
    const tree = await this.deckTree();
    const deckItem = (n: T.DeckTreeNode): T.SidebarItem => ({ kind: 'deck', label: n.deck.name.split('::').pop()!, query: `deck:"${n.deck.name}"`, count: n.totalCards, children: n.children.map(deckItem) });
    const tags = await this.allTags();
    const flags = ['Red', 'Orange', 'Green', 'Blue', 'Pink', 'Turquoise', 'Purple'];
    return [
      { kind: 'saved', label: 'Saved searches', query: '', children: this.saved.map((s) => ({ kind: 'saved' as const, label: s.name, query: s.query })) },
      { kind: 'today', label: 'Today', query: '', children: [
        { kind: 'today', label: 'Due', query: 'is:due', count: tree.reviewCount + tree.learnCount },
        { kind: 'today', label: 'Added today', query: 'added:1', count: 0 },
        { kind: 'today', label: 'Studied today', query: 'rated:1', count: 42 },
        { kind: 'today', label: 'Forgotten today', query: 'rated:1:1', count: 5 },
      ] },
      { kind: 'flag', label: 'Flags', query: '', children: flags.map((f, i) => ({ kind: 'flag' as const, label: f, query: `flag:${i + 1}` })) },
      { kind: 'deck', label: 'Decks', query: '', children: tree.children.map(deckItem) },
      { kind: 'notetype', label: 'Note types', query: '', children: [...this.notetypes.values()].map((n) => ({ kind: 'notetype' as const, label: n.name, query: `note:"${n.name}"` })) },
      { kind: 'tag', label: 'Tags', query: '', children: tags.map((t) => ({ kind: 'tag' as const, label: t.tag, query: `tag:${t.tag}`, count: t.count })) },
    ];
  }
  async bulk(cardIds: number[], action: T.BulkAction) {
    await this.wait(140);
    const snap = this.snapshot();
    const cards = cardIds.map((id) => this.cards.get(id)).filter((c): c is T.Card => !!c);
    const notes = [...new Set(cards.map((c) => c.nid))].map((id) => this.notes.get(id)!).filter(Boolean);
    let label = 'Change';
    switch (action.kind) {
      case 'changeDeck': cards.forEach((c) => (c.did = action.deckId)); label = 'Change deck'; break;
      case 'addTags': notes.forEach((n) => { n.tags = [...new Set([...n.tags, ...action.tags])]; }); label = 'Add tags'; break;
      case 'removeTags': notes.forEach((n) => { n.tags = n.tags.filter((t) => !action.tags.includes(t)); }); label = 'Remove tags'; break;
      case 'suspend': cards.forEach((c) => (c.queue = CardQueue.Suspended)); label = 'Suspend'; break;
      case 'unsuspend': cards.forEach((c) => (c.queue = c.type as number as CardQueue)); label = 'Unsuspend'; break;
      case 'bury': cards.forEach((c) => (c.queue = CardQueue.UserBuried)); label = 'Bury'; break;
      case 'unbury': cards.forEach((c) => (c.queue = c.type as number as CardQueue)); label = 'Unbury'; break;
      case 'flag': cards.forEach((c) => (c.flags = action.flag)); label = 'Flag'; break;
      case 'forget': cards.forEach((c) => { c.type = CardType.New; c.queue = CardQueue.New; c.ivl = 0; if (action.resetCounts) { c.reps = 0; c.lapses = 0; } }); label = 'Forget'; break;
      case 'delete': notes.forEach((n) => { this.notes.delete(n.id); for (const c of [...this.cards.values()]) if (c.nid === n.id) this.cards.delete(c.id); }); label = 'Delete'; break;
      case 'findReplace': {
        const re = action.regex ? new RegExp(action.find, action.matchCase ? 'g' : 'gi') : new RegExp(action.find.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), action.matchCase ? 'g' : 'gi');
        notes.forEach((n) => { n.fields = n.fields.map((f) => f.replace(re, action.replace)); });
        label = 'Find and replace'; break;
      }
      case 'reposition': label = 'Reposition'; break;
      case 'setDue': cards.forEach((c) => { c.type = CardType.Review; c.queue = CardQueue.Review; c.due = TODAY + (parseInt(action.days, 10) || 0); }); label = 'Set due date'; break;
    }
    this.pushUndo(label, () => this.restore(snap));
    this.changed('cards', 'notes', 'decks');
    return { affected: cards.length, undoLabel: label };
  }
  async saveSearch(name: string, query: string) { await this.wait(40); this.saved = [...this.saved.filter((s) => s.name !== name), { name, query }]; this.changed('tags'); }
  async deleteSavedSearch(name: string) { await this.wait(40); this.saved = this.saved.filter((s) => s.name !== name); this.changed('tags'); }

  // ───────── stats ─────────
  async stats(_req: T.StatsRequest): Promise<T.Stats> {
    await this.wait(220);
    const r = mulberry32(365);
    const days = 365;
    const reviewsPerDay: T.Stats['reviewsPerDay'] = [];
    const heatmap: T.Stats['heatmap'] = [];
    for (let i = days - 1; i >= 0; i--) {
      const day = TODAY - i;
      const season = 0.75 + 0.25 * Math.sin((i / 365) * Math.PI * 2 + 1);
      const weekend = ((day + 1) % 7) >= 5 ? 0.6 : 1;
      const skip = r() < (i > 200 ? 0.32 : 0.14);
      const count = skip ? 0 : Math.round((22 + r() * 70) * season * weekend * (i < 20 ? 1.2 : 1));
      const learn = Math.round(count * 0.12), relearn = Math.round(count * 0.07);
      reviewsPerDay.push({ day, count, seconds: count * (6 + r() * 3), learn, review: count - learn - relearn, relearn });
      heatmap.push({ date: new Date((CRT_S + day * DAY_S) * 1000).toISOString().slice(0, 10), count });
    }
    const last30 = reviewsPerDay.slice(-30);
    let run = 0; for (let i = reviewsPerDay.length - 1; i >= 0 && reviewsPerDay[i]!.count > 0; i--) run++;
    let longest = 0, cur = 0; for (const d of reviewsPerDay) { cur = d.count > 0 ? cur + 1 : 0; longest = Math.max(longest, cur); }
    const tree = await this.deckTree();
    return {
      today: { studied: 42, seconds: 512, again: 5, correctPct: 0.88, learned: 9, relearned: 3 },
      forecast: Array.from({ length: 90 }, (_, d) => { const due = Math.round(tree.reviewCount * Math.exp(-d / 40) * 0.6 + 20 + r() * 18); return { day: d, due, withNew: due + Math.round(12 * (1 - Math.exp(-d / 20))) }; }),
      retention: [
        { label: '7 days', trueRetention: 0.91, desired: 0.9, reviews: 312 },
        { label: '30 days', trueRetention: 0.87, desired: 0.9, reviews: 1388 },
        { label: '90 days', trueRetention: 0.88, desired: 0.9, reviews: 4102 },
        { label: '1 year', trueRetention: 0.86, desired: 0.9, reviews: 15920 },
        { label: 'All time', trueRetention: 0.85, desired: 0.9, reviews: 48213 },
      ],
      reviewsPerDay, heatmap,
      intervals: [['1d', 210], ['2–3d', 260], ['4–7d', 380], ['1–2w', 420], ['2–4w', 610], ['1–3mo', 880], ['3–6mo', 640], ['6–12mo', 420], ['1y+', 190]].map(([bucket, count]) => ({ bucket: bucket as string, count: count as number })),
      stability: [['<1d', 120], ['1–3d', 240], ['3–7d', 360], ['1–2w', 470], ['2–4w', 650], ['1–3mo', 820], ['3–6mo', 700], ['6–12mo', 450], ['1y+', 260]].map(([bucket, count]) => ({ bucket: bucket as string, count: count as number })),
      difficulty: Array.from({ length: 10 }, (_, i) => ({ bucket: `${i * 10}–${i * 10 + 10}%`, count: Math.round(420 * Math.exp(-Math.pow((i - 4.2) / 2.2, 2)) + 20) })),
      hourly: Array.from({ length: 24 }, (_, h) => ({ hour: h, reviews: h < 6 ? Math.round(r() * 12) : Math.round(80 + 400 * Math.exp(-Math.pow((h - (h < 14 ? 8 : 21)) / 2.5, 2)) + r() * 60), correctPct: 0.82 + 0.08 * Math.exp(-Math.pow((h - 9) / 4, 2)) + r() * 0.02 })),
      buttons: { learn: [412, 88, 1830, 140], young: [620, 240, 4810, 380], mature: [880, 410, 13240, 1210] },
      cardCounts: { new: 1420, learning: 64, young: 980, mature: 1802, suspended: 41, buried: 7 },
      momentum: { daysStudiedLast30: last30.filter((d) => d.count > 0).length, currentRun: run, longestRun: longest },
      insights: await this.insights(),
    };
  }
  async insights(): Promise<T.Insight[]> {
    await this.wait(90);
    if (this.decks.size === 0) return [];
    return [
      { id: 'retention-gap', severity: 'suggest', text: 'True retention over 30 days is 87%, below your 90% target. Optimising parameters for “Languages” should close most of the gap.', action: { label: 'Optimise parameters', route: '#/settings/presets/2' } },
      { id: 'struggling', severity: 'warn', text: 'Pharmacology has 6 struggling cards that keep slipping. Rewording them usually helps more than repeating them.', action: { label: 'See the cards', route: '#/browse?q=' + encodeURIComponent('deck:"Medicine::Pharmacology" tag:leech') } },
      { id: 'pace', severity: 'info', text: 'At your current pace the backlog clears in 3 days; a catch-up plan can make it 2 without risking forgotten cards.', action: { label: 'Plan a catch-up', route: '#/backlog/all' } },
    ];
  }

  // ───────── scheduler maintenance ─────────
  async optimise(configId: number, onProgress?: (p: T.Progress) => void): Promise<T.OptimiseResult> {
    const p = this.presets.get(configId);
    if (!p) throw new LociError('not-found', 'Preset not found.');
    const total = 12;
    for (let i = 0; i <= total; i++) { this.progress('optimise', { phase: i < 2 ? 'Reading review history' : i < total ? 'Fitting parameters' : 'Done', done: i, total }, onProgress); await sleep(140); }
    const before = [...p.config.fsrsParams];
    const r = mulberry32(configId * 97);
    const after = before.map((w, i) => (i === 20 ? w : Math.round(w * (0.92 + r() * 0.16) * 10000) / 10000));
    p.config.fsrsParams = after;
    p.mtime = NOW_S;
    const result: T.OptimiseResult = { configId, reviews: 12430, before: { params: before, logLoss: 0.3311, rmse: 0.0521 }, after: { params: after, logLoss: 0.3087, rmse: 0.0346 }, adopted: true, reason: 'The new parameters predict your answers better, so they were adopted.' };
    this.events.emit({ type: 'optimised', result });
    this.changed('presets');
    return clone(result);
  }
  async evaluate(configId: number) { await this.wait(600); void configId; return { logLoss: 0.3087, rmse: 0.0346, reviews: 12430 }; }
  async maintenance() { await this.wait(60); return { optimised: [], backedUp: false }; }

  // ───────── import / export ─────────
  async previewImport(bytes: Uint8Array): Promise<T.ImportPreview> {
    await this.wait(650);
    if (bytes.length === 0) throw new LociError('corrupt', 'The file is empty.');
    const head = String.fromCharCode(...bytes.slice(0, 3));
    if (head === 'BAD') throw new LociError('unsupported-package', 'This file is not an Anki package.');
    if (head === 'COR') throw new LociError('corrupt', 'The package is damaged: the collection inside could not be read.');
    const n = 1200 + (bytes.length % 800);
    return {
      layout: 'latest', isCollectionPackage: false, notes: n, cards: Math.round(n * 1.6), decks: ['Japanese', 'Japanese::Core 2k', 'Japanese::Kanji', 'Japanese::Grammar'],
      notetypes: ['Basic', 'Cloze', 'Japanese (recognition)'], reviews: 48213, media: 864, hasScheduling: true, fsrsCards: Math.round(n * 1.5), sourceCrt: CRT_S - 400 * DAY_S, warnings: [],
      notesExisting: 0, notesNew: n,
    };
  }
  async importPackage(bytes: Uint8Array, options: T.ImportOptions, onProgress?: (p: T.Progress) => void): Promise<T.ImportReport> {
    const preview = await this.previewImport(bytes);
    const phases: Array<[string, number]> = [['Reading package', 4], ['Notes', preview.notes], ['Cards', preview.cards], ['Review history', preview.reviews], ['Media', preview.media], ['Checking integrity', 3]];
    for (const [phase, total] of phases) {
      for (let k = 0; k <= 5; k++) { this.progress('import', { phase, done: Math.round((total * k) / 5), total }, onProgress); await sleep(70); }
    }
    this.progress('import', { phase: 'Done', done: 1, total: 1 }, onProgress);
    if (options.mode === 'replace') { this.decks.clear(); this.deckCounts.clear(); this.notes.clear(); this.cards.clear(); }
    if (this.decks.size === 0) this.seedCollection();
    const top = this.ensureDeckPath('Japanese', 1);
    this.deckCounts.set(top.id, { new: 0, learn: 0, review: 4, total: 120 });
    for (const [name, c] of [['Japanese::Core 2k', { new: 10, learn: 2, review: 31, total: 2000 }], ['Japanese::Kanji', { new: 5, learn: 0, review: 18, total: 840 }], ['Japanese::Grammar', { new: 0, learn: 1, review: 7, total: 192 }]] as const) {
      const d = this.ensureDeckPath(name, 1);
      this.deckCounts.set(d.id, { ...c });
    }
    this.changed('decks', 'notes', 'cards', 'notetypes', 'presets');
    return {
      notesAdded: preview.notes - 40, notesUpdated: 40, notesSkipped: 0, cardsAdded: preview.cards - 64, reviewsAdded: options.keepScheduling ? preview.reviews : 0,
      mediaAdded: preview.media, decksAdded: preview.decks.length, notetypesAdded: 1, warnings: preview.media > 800 ? ['3 media files were referenced but missing from the package.'] : [], durationMs: 2480,
    };
  }
  async exportPackage(options: T.ExportOptions, onProgress?: (p: T.Progress) => void) {
    for (let k = 0; k <= 6; k++) { this.progress('export', { phase: k < 3 ? 'Collecting notes' : k < 6 ? 'Writing media' : 'Done', done: k, total: 6 }, onProgress); await sleep(90); }
    return new TextEncoder().encode(`PK\u0003\u0004 mock apkg ${options.deckId ?? 'all'} sched=${options.includeScheduling}`);
  }
  async installSampleDeck() {
    await this.wait(600);
    if (this.decks.size === 0) this.seedCollection();
    this.changed('decks', 'notes', 'cards');
    return 1001;
  }

  // ───────── backups ─────────
  async listBackups() { await this.wait(90); return clone([...this.backups].sort((a, b) => b.at - a.at)); }
  async backupNow(): Promise<T.BackupInfo> {
    await this.wait(900);
    const at = Math.floor(Date.now() / 1000);
    const info: T.BackupInfo = { name: `loci-${new Date(at * 1000).toISOString().slice(0, 19).replace(/[:T]/g, '-')}-manual.sqlite`, at, bytes: 9_512_330, cards: 4368, kind: 'manual' };
    this.backups.push(info);
    this.events.emit({ type: 'backup', info });
    this.changed('backups');
    return clone(info);
  }
  async restoreBackup(name: string): Promise<T.IntegrityReport> {
    if (!this.backups.some((b) => b.name === name)) throw new LociError('not-found', 'That backup no longer exists.');
    for (let k = 0; k <= 4; k++) { this.progress('restore', { phase: k === 0 ? 'Backing up the current collection' : 'Restoring', done: k, total: 4 }, undefined); await sleep(220); }
    const at = Math.floor(Date.now() / 1000);
    this.backups.push({ name: `loci-${at}-pre-restore.sqlite`, at, bytes: 9_500_000, cards: 4368, kind: 'pre-restore' });
    this.changed('backups', 'decks', 'notes', 'cards');
    return { ok: true, problems: [], repaired: [] };
  }
  async exportFullBackup(onProgress?: (p: T.Progress) => void) {
    for (let k = 0; k <= 5; k++) { this.progress('backup-export', { phase: k < 2 ? 'Collection' : 'Media', done: k, total: 5 }, onProgress); await sleep(110); }
    return new TextEncoder().encode('PK\u0003\u0004 mock colpkg');
  }
  async checkIntegrity(): Promise<T.IntegrityReport> { await this.wait(900); return { ok: true, problems: [], repaired: ['Rebuilt the search index (2 notes were missing from it).'] }; }
  async resetCollection() {
    await this.wait(500);
    this.decks.clear(); this.deckCounts.clear(); this.notes.clear(); this.cards.clear(); this.revlog = []; this.queue = []; this.undoStack = [];
    this.changed('decks', 'notes', 'cards', 'tags', 'backups');
  }

  // ───────── sync ─────────
  private syncFolder: string | null = null;
  private lastSync: number | null = null;
  async syncStatus(): Promise<T.SyncStatus> {
    await this.wait(40);
    return { enabled: this.syncFolder !== null || this.lastSync !== null, deviceId: 'a1b2c3d4e5f60718', collectionId: 'mock', folder: this.syncFolder, lastSync: this.lastSync, peers: [], formerDevices: [] };
  }
  async setSyncFolder(folder: string | null): Promise<T.SyncStatus> { this.syncFolder = folder; return this.syncStatus(); }
  async exportSyncBundle(): Promise<Uint8Array> { await this.wait(300); return new TextEncoder().encode('PK\u0003\u0004 mock sync'); }
  async mergeSyncBundle(): Promise<T.SyncReport> {
    await this.wait(400);
    this.lastSync = Math.floor(Date.now() / 1000);
    this.changed('decks', 'notes', 'cards');
    return { fromDevice: '0f1e2d3c4b5a6978', rowsIn: 12, conflictsResolved: 0, reviewsAdded: 30, mediaAdded: 0, mediaNeeded: [], complete: true };
  }
  async syncMediaState(): Promise<T.SyncMediaState> { return { have: [], need: [] }; }
  async addSyncMedia(): Promise<number> { return 0; }
}

/** Mock implementation of the worker client contract. */
export async function createMockApi(opts: { empty?: boolean } = {}): Promise<ApiHandle> {
  const events = new Emitter();
  const api = new MockCollection(events, !!opts.empty);
  await sleep(60);
  return { api, events };
}

export { STANDARD_CONFIG };
