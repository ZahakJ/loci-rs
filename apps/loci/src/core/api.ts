/**
 * CollectionApi — the single interface the UI calls. Implemented by `Collection`
 * (src/core/collection) directly in Node/tests, and by the worker RPC client in the
 * browser (every method becomes `Promise<T>`; `Progress` callbacks become events).
 *
 * Conventions: methods throw `LociError` (code + message) on failure; they never
 * return partial writes. Any method marked (undoable) pushes an undo entry.
 */
import type * as T from './types';

export interface CollectionApi {
  // ── lifecycle ──
  /** Opens (creating if needed), runs migrations and integrity_check. */
  open(): Promise<T.IntegrityReport>;
  close(): Promise<void>;
  isEmpty(): Promise<boolean>;
  getSettings(): Promise<T.CollectionSettings>;
  setSettings(patch: Partial<T.CollectionSettings>): Promise<T.CollectionSettings>;
  /** Collection day number for "now" (respects rollover hour). */
  today(): Promise<T.CollectionDay>;

  // ── decks & presets ──
  deckTree(): Promise<T.DeckTreeNode>;                               // root = virtual "All", children = top-level decks
  getDeck(id: T.DeckId): Promise<T.Deck>;
  createDeck(name: string, configId?: T.DeckConfigId): Promise<T.Deck>;
  renameDeck(id: T.DeckId, name: string): Promise<void>;
  deleteDeck(id: T.DeckId): Promise<{ cards: number }>;              // (undoable)
  setDeckCollapsed(id: T.DeckId, collapsed: boolean): Promise<void>;
  setDeckPreset(id: T.DeckId, configId: T.DeckConfigId): Promise<void>;
  setDeckDescription(id: T.DeckId, description: string): Promise<void>;
  listPresets(): Promise<T.DeckConfigPreset[]>;
  getPreset(id: T.DeckConfigId): Promise<T.DeckConfigPreset>;
  savePreset(preset: Omit<T.DeckConfigPreset, 'mtime' | 'usn'> & { id?: T.DeckConfigId }): Promise<T.DeckConfigPreset>;
  deletePreset(id: T.DeckConfigId): Promise<void>;                   // decks fall back to preset 1
  /** Simulates the next 90 days for a preset change (actual deck contents). */
  forecastWorkload(deckId: T.DeckId | null, config: T.DeckConfig): Promise<T.WorkloadForecast>;

  // ── note types ──
  listNotetypes(): Promise<T.Notetype[]>;
  getNotetype(id: T.NotetypeId): Promise<T.Notetype>;
  saveNotetype(nt: Omit<T.Notetype, 'mtime' | 'usn'> & { id?: T.NotetypeId }): Promise<T.Notetype>; // regenerates cards
  deleteNotetype(id: T.NotetypeId): Promise<void>;                   // refuses if notes exist
  /** Renders an arbitrary note against a note type for live preview (no persistence). */
  previewNote(ntid: T.NotetypeId, fields: string[], ord: number): Promise<T.RenderedCard>;
  /** Validates template syntax; returns human messages. */
  checkTemplate(ntid: T.NotetypeId, qfmt: string, afmt: string): Promise<string[]>;

  // ── notes & cards ──
  getNote(id: T.NoteId): Promise<T.Note>;
  getCard(id: T.CardId): Promise<T.Card>;
  getCardsOfNote(id: T.NoteId): Promise<T.Card[]>;
  /** Adds a note; generates cards per template rules; returns ids. (undoable) */
  addNote(ntid: T.NotetypeId, deckId: T.DeckId, fields: string[], tags: string[]): Promise<{ noteId: T.NoteId; cardIds: T.CardId[] }>;
  /** Updates fields/tags; adds or removes generated cards as needed. (undoable) */
  updateNote(id: T.NoteId, fields: string[], tags: string[]): Promise<{ cardsAdded: number; cardsRemoved: number }>;
  deleteNotes(ids: T.NoteId[]): Promise<void>;                       // (undoable)
  /** First-field duplicate check; returns note ids with same csum+stripped text in same note type. */
  findDuplicates(ntid: T.NotetypeId, firstField: string): Promise<T.NoteId[]>;
  renderCard(id: T.CardId): Promise<T.RenderedCard>;
  explainCard(id: T.CardId): Promise<T.CardExplanation>;
  allTags(): Promise<Array<{ tag: string; count: number }>>;
  renameTag(from: string, to: string): Promise<number>;             // (undoable)

  // ── media ──
  /** Stores bytes under a normalised unique name; returns the name to reference in a field. */
  addMedia(name: string, bytes: Uint8Array): Promise<string>;
  getMedia(name: string): Promise<Uint8Array | null>;
  /** Unused files and missing references. */
  checkMedia(): Promise<{ unused: string[]; missing: string[] }>;
  deleteMedia(names: string[]): Promise<void>;

  // ── study ──
  /** Starts/continues a session for a deck (null = all decks); returns the first card or null if nothing due. */
  startStudy(deckId: T.DeckId | null): Promise<T.StudyCard | null>;
  nextCard(): Promise<T.StudyCard | null>;
  answerCard(cardId: T.CardId, rating: T.Rating, elapsedMs: number): Promise<T.AnswerResult>; // (undoable)
  /** Custom session: study ahead N days, review forgotten in N days, preview new, or an arbitrary search. */
  startCustomStudy(deckId: T.DeckId | null, kind: { kind: 'ahead'; days: number } | { kind: 'forgotten'; days: number } | { kind: 'preview' } | { kind: 'search'; query: string; limit: number }): Promise<T.StudyCard | null>;
  endStudy(): Promise<T.SessionSummary>;
  suspendCards(ids: T.CardId[], suspend: boolean): Promise<void>;    // (undoable)
  buryCards(ids: T.CardId[]): Promise<void>;                         // (undoable)
  setFlag(ids: T.CardId[], flag: T.Flag): Promise<void>;
  unburyDeck(deckId: T.DeckId | null): Promise<number>;
  /** Struggling-card flow: 'rewrite' opens editor (UI), these are the data actions. */
  resolveLeech(cardId: T.CardId, action: 'reset' | 'suspend' | 'keep'): Promise<void>;

  // ── undo ──
  undo(): Promise<{ label: string } | null>;
  undoLabel(): Promise<string | null>;

  // ── backlog ──
  backlogReport(deckId: T.DeckId | null): Promise<T.BacklogReport>;
  applyCatchUp(deckId: T.DeckId | null, plan: T.CatchUpPlan): Promise<T.CatchUpResult>; // (undoable)

  // ── browse ──
  browse(req: T.BrowseRequest): Promise<T.BrowseResult>;
  /** Returns all matching ids (for select-all and bulk actions). */
  searchIds(query: string, mode: T.BrowseMode): Promise<Array<T.CardId | T.NoteId>>;
  sidebar(): Promise<T.SidebarItem[]>;
  bulk(cardIds: T.CardId[], action: T.BulkAction): Promise<{ affected: number; undoLabel: string }>; // (undoable)
  saveSearch(name: string, query: string): Promise<void>;
  deleteSavedSearch(name: string): Promise<void>;

  // ── stats ──
  stats(req: T.StatsRequest): Promise<T.Stats>;
  insights(): Promise<T.Insight[]>;

  // ── scheduler maintenance ──
  optimise(configId: T.DeckConfigId, onProgress?: (p: T.Progress) => void): Promise<T.OptimiseResult>;
  evaluate(configId: T.DeckConfigId): Promise<{ logLoss: number; rmse: number; reviews: number }>;
  /** Runs auto-optimisation where due; returns what changed. Called by the UI on idle. */
  maintenance(): Promise<{ optimised: T.OptimiseResult[]; backedUp: boolean }>;

  // ── import / export ──
  previewImport(bytes: Uint8Array): Promise<T.ImportPreview>;
  importPackage(bytes: Uint8Array, options: T.ImportOptions, onProgress?: (p: T.Progress) => void): Promise<T.ImportReport>;
  exportPackage(options: T.ExportOptions, onProgress?: (p: T.Progress) => void): Promise<Uint8Array>;
  /** Builds the bundled sample deck (30 cards about memory). */
  installSampleDeck(): Promise<T.DeckId>;

  // ── backups ──
  listBackups(): Promise<T.BackupInfo[]>;
  backupNow(kind?: 'manual'): Promise<T.BackupInfo>;
  restoreBackup(name: string): Promise<T.IntegrityReport>;           // current collection is backed up as 'pre-restore' first
  /** Full backup package: collection + media as a .colpkg-compatible zip. */
  exportFullBackup(onProgress?: (p: T.Progress) => void): Promise<Uint8Array>;
  checkIntegrity(): Promise<T.IntegrityReport>;
  /** Danger zone: wipe and start empty (UI confirms twice; a pre-restore backup is kept). */
  resetCollection(): Promise<void>;

  // ── appended (fix round) ──
  /**
   * Anki's "increase today's limits": lets `extraNew` more new cards and `extraReview` more
   * reviews be served today in the deck and its subdecks (lowers decks.today counters; resets
   * with the day). Returns the deck's counts after the change. (undoable)
   */
  extendTodayLimits(deckId: T.DeckId, extraNew: number, extraReview: number): Promise<T.StudyCounts>;

  // ── sync (docs/SYNC.md) ──
  syncStatus(): Promise<T.SyncStatus>;
  /** Remembers this device's sync folder (null forgets it); turns sync on. */
  setSyncFolder(folder: string | null): Promise<T.SyncStatus>;
  /** This device's sync file. */
  exportSyncBundle(options?: T.SyncExportOptions): Promise<Uint8Array>;
  /** Merges another device's sync file (an automatic backup is written first). */
  mergeSyncBundle(bytes: Uint8Array): Promise<T.SyncReport>;
  syncMediaState(): Promise<T.SyncMediaState>;
  /** Stores a media file fetched from another device by its SHA-1; returns files written. */
  addSyncMedia(sha1: string, bytes: Uint8Array): Promise<number>;
}

export class LociError extends Error {
  constructor(public code: LociErrorCode, message: string, public detail?: unknown) { super(message); this.name = 'LociError'; }
}
export type LociErrorCode =
  | 'not-found' | 'invalid' | 'conflict' | 'unsupported-package' | 'corrupt' | 'io' | 'busy' | 'template' | 'media' | 'internal';

/** Events the worker emits to the UI (besides method returns). */
export type CollectionEvent =
  | { type: 'progress'; op: string; progress: T.Progress }
  | { type: 'changed'; what: Array<'decks' | 'notes' | 'cards' | 'notetypes' | 'presets' | 'settings' | 'tags' | 'backups'> }
  | { type: 'backup'; info: T.BackupInfo }
  | { type: 'optimised'; result: T.OptimiseResult }
  | { type: 'error'; error: { code: LociErrorCode; message: string } };
