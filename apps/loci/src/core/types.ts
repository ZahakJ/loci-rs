/**
 * Loci domain types — the contract shared by core, worker and UI.
 * Units follow Anki (see schema.sql header). Ids are numbers (ms epoch), never strings.
 */

// ───────────────────────── identifiers & time ─────────────────────────
export type NoteId = number;
export type CardId = number;
export type DeckId = number;
export type NotetypeId = number;
export type DeckConfigId = number;
export type RevlogId = number;
/** Seconds since epoch. */
export type Seconds = number;
/** Milliseconds since epoch. */
export type Millis = number;
/** Days since the collection's creation day (crt). */
export type CollectionDay = number;

export const FIELD_SEPARATOR = '\u001f';

// ───────────────────────── enums (Anki values) ─────────────────────────
export enum CardType { New = 0, Learn = 1, Review = 2, Relearn = 3 }
export enum CardQueue {
  UserBuried = -3, SchedBuried = -2, Suspended = -1,   // Anki: -2 = buried by sibling burying, -3 = buried manually
  New = 0, Learn = 1, Review = 2, DayLearn = 3, Preview = 4,
}
export enum Rating { Again = 1, Hard = 2, Good = 3, Easy = 4 }
export enum RevlogType { Learn = 0, Review = 1, Relearn = 2, Filtered = 3, Manual = 4, Rescheduled = 5 }
export enum NotetypeKind { Standard = 0, Cloze = 1 }
export enum GraveType { Card = 0, Note = 1, Deck = 2 }
export type Flag = 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7; // 0 none, 1 red, 2 orange, 3 green, 4 blue, 5 pink, 6 turquoise, 7 purple

// ───────────────────────── note types ─────────────────────────
export interface FieldConfig {
  sticky: boolean;
  rtl: boolean;
  font: string;          // '' = inherit the app card font
  size: number;          // 0 = inherit
  description: string;   // placeholder shown in the editor
  plainText: boolean;    // edit as plain text (no HTML)
  collapsed: boolean;
}
export interface Field { ord: number; name: string; config: FieldConfig }

export interface TemplateConfig { deckOverride?: DeckId }
export interface Template {
  ord: number; name: string;
  qfmt: string; afmt: string; bqfmt: string; bafmt: string;
  config: TemplateConfig;
}

export interface NotetypeConfig {
  /** Anki's card-generation requirements, kept verbatim for export: [ord, 'all'|'any'|'none', fieldOrds[]][] */
  req: Array<[number, 'all' | 'any' | 'none', number[]]>;
  /** One of the built-in kinds, if this note type was created by Loci. */
  builtin?: 'basic' | 'basic-reversed' | 'basic-typed' | 'cloze' | 'image-occlusion';
  /** Cloze: reveal clozes one by one on the back. */
  clozeOneByOne?: boolean;
  /** Image occlusion: 'hide-all' (hide all shapes, reveal one) or 'hide-one'. */
  occlusionMode?: 'hide-all' | 'hide-one';
  /** Anki id this note type was imported from, if different (collisions). */
  originalId?: number;
  /** Anki `originalStockKind` (1 basic, 2 basic+reversed, 3 optional reversed, 4 typing, 5 cloze, 6 image occlusion); kept for export. */
  originalStockKind?: number;
  /**
   * (appended) Keep this note type's own colours and fonts: when true its CSS is used as written;
   * otherwise Anki's stock rules and the card root's colour/background/font declarations are
   * dropped so Settings and the theme decide (core/template/css.ts).
   */
  useOwnStyling?: boolean;
}
export interface Notetype {
  id: NotetypeId; name: string; kind: NotetypeKind;
  css: string; latexPre: string; latexPost: string; sortField: number;
  fields: Field[]; templates: Template[];
  config: NotetypeConfig; mtime: Seconds; usn: number;
}

// ───────────────────────── decks & presets ─────────────────────────
export interface DeckToday { day: CollectionDay; new: number; rev: number; lrn: number; timeMs: number }
export interface Deck {
  id: DeckId; name: string; configId: DeckConfigId; description: string;
  collapsed: boolean; today: DeckToday; mtime: Seconds; usn: number;
}
export interface DeckTreeNode {
  deck: Deck; children: DeckTreeNode[]; level: number;
  /** counts for today after limits, including children */
  newCount: number; learnCount: number; reviewCount: number;
  /** estimated minutes to clear, from recent seconds-per-card */
  estimatedMinutes: number;
  totalCards: number;
}

export type SchedulerKind = 'fsrs' | 'sm2';
export type NewCardOrder = 'due' | 'random';
export type NewCardGather = 'deck' | 'deck-random-notes' | 'ascending' | 'descending' | 'random-notes' | 'random-cards';
export type NewCardSort = 'template-then-order' | 'order' | 'template-then-random' | 'random-note' | 'random-card';
export type ReviewOrder = 'due-then-random' | 'due-then-deck' | 'deck-then-due' | 'ascending-intervals' | 'descending-intervals' | 'ascending-ease' | 'descending-ease' | 'relative-overdue' | 'random' | 'added' | 'reverse-added';
export type NewReviewMix = 'mix' | 'review-first' | 'new-first';
export type LeechAction = 'suspend' | 'tag';

export interface DeckConfig {
  scheduler: SchedulerKind;
  /** Daily limits */
  newPerDay: number;
  reviewsPerDay: number;
  /** Loci: minutes target per day used for Home estimates and catch-up (not a hard cap). */
  minutesPerDay: number;
  /** Learning (minutes; fractional allowed) */
  learnSteps: number[];
  relearnSteps: number[];
  graduatingInterval: number;      // days, SM-2
  easyInterval: number;            // days, SM-2
  newOrder: NewCardOrder;
  newGather: NewCardGather;
  newSort: NewCardSort;
  reviewOrder: ReviewOrder;
  newReviewMix: NewReviewMix;
  interdayLearningMix: NewReviewMix;
  /** SM-2 */
  initialEase: number;             // permille, 2500
  easyBonus: number;               // 1.3
  intervalModifier: number;        // 1.0
  hardMultiplier: number;          // 1.2
  lapseMultiplier: number;         // 0.0
  minimumLapseInterval: number;    // days, 1
  maximumInterval: number;         // days, 36500
  /** FSRS */
  fsrsParams: number[];            // 21 numbers (17/19 accepted and upgraded)
  desiredRetention: number;        // 0.70–0.99
  historicalRetention: number;     // used for SM-2 → FSRS conversion
  autoOptimise: boolean;
  optimiseEveryReviews: number;    // 400
  /** Lapses */
  leechThreshold: number;          // 8
  leechAction: LeechAction;
  /** Burying */
  burySiblingsNew: boolean;
  burySiblingsReview: boolean;
  burySiblingsInterday: boolean;
  /** Audio */
  autoplayAudio: boolean;
  replayQuestionAudio: boolean;
  /** Timer */
  maxAnswerSeconds: number;        // 60
  showTimer: boolean;
  /** Display */
  showIntervalsOnButtons: boolean;
}
export interface DeckConfigPreset { id: DeckConfigId; name: string; config: DeckConfig; mtime: Seconds; usn: number }

// ───────────────────────── notes & cards ─────────────────────────
export interface Note {
  id: NoteId; guid: string; ntid: NotetypeId; mtime: Seconds; usn: number;
  tags: string[]; fields: string[];
}
export interface CardData {
  /** FSRS stability (days) */ s?: number;
  /** FSRS difficulty (1..10) */ d?: number;
  /** desired retention at last scheduling */ dr?: number;
  /** FSRS-6 decay */ decay?: number;
  /** original new-card position */ pos?: number;
  /** custom data (Anki `cd`) */ cd?: string;
  /** last review time, unix seconds (Anki `lrt`) */ lrt?: number;
}
export interface Card {
  id: CardId; nid: NoteId; did: DeckId; ord: number; mtime: Seconds; usn: number;
  type: CardType; queue: CardQueue; due: number; ivl: number; factor: number;
  reps: number; lapses: number; left: number; odue: number; odid: DeckId; flags: number;
  data: CardData;
}
export interface RevlogEntry {
  id: RevlogId; cid: CardId; usn: number; ease: number; ivl: number; lastIvl: number;
  factor: number; time: number; type: RevlogType;
}

// ───────────────────────── rendering ─────────────────────────
export interface RenderedSide {
  /** sanitised HTML for the shadow root */ html: string;
  /** audio/video filenames in play order ([sound:] tags) */ media: string[];
  /** true when the side contains a {{type:}} field */ hasTypeInput: boolean;
  /** expected typed answer (plain text) when hasTypeInput */ typeAnswer?: string;
  /** cloze ordinals present on this card (for one-by-one) */ clozeOrds?: number[];
}
export interface RenderedCard {
  cardId: CardId; noteId: NoteId; notetypeId: NotetypeId; templateName: string;
  front: RenderedSide; back: RenderedSide;
  /** CSS from the note type, already scoped for the shadow root */ css: string;
  /** direction hint for the whole card */ dir: 'auto' | 'rtl' | 'ltr';
  /** image occlusion payload when applicable */ occlusion?: OcclusionPayload;
}
export interface OcclusionShape {
  kind: 'rect' | 'ellipse' | 'polygon' | 'text';
  ord: number;                 // cloze ordinal
  /** normalised 0..1 coordinates relative to the image */
  left?: number; top?: number; width?: number; height?: number;
  points?: Array<[number, number]>;
  text?: string;
}
export interface OcclusionPayload { image: string; shapes: OcclusionShape[]; mode: 'hide-all' | 'hide-one'; activeOrd: number }

// ───────────────────────── study ─────────────────────────
export interface StudyCounts { new: number; learn: number; review: number }
export interface SchedulingPreview {
  /** human interval label per rating, e.g. '<10m', '3d', '1.2mo' */
  labels: Record<Rating, string>;
  /** seconds until each rating's next due */
  secondsUntil: Record<Rating, number>;
}
export interface StudyCard {
  card: Card; note: Note; rendered: RenderedCard; preview: SchedulingPreview;
  counts: StudyCounts; deckName: string; isLeech: boolean;
}
export interface AnswerResult {
  counts: StudyCounts; next: StudyCard | null; undoLabel: string;
  /** set when the answer made the card a leech */ becameLeech?: boolean;
  /** true when the session has nothing left */ finished: boolean;
}
export interface CardExplanation {
  retrievability: number | null; stability: number | null; difficulty: number | null;
  dueDate: Seconds | null; interval: number; reps: number; lapses: number; averageSeconds: number;
  history: RevlogEntry[]; sentence: string;
}
export interface SessionSummary {
  cards: number; seconds: number; accuracy: number; again: number; hard: number; good: number; easy: number;
  tomorrowDue: number; insight?: Insight;
}

// ───────────────────────── backlog / catch-up ─────────────────────────
export interface BacklogReport {
  due: number; overdue: number; maxOverdueDays: number; dailyAverage: number;
  estimatedMinutes: number; daysAtCurrentPace: number; triggered: boolean;
}
export type CatchUpPlan =
  | { kind: 'spread'; days: number }
  | { kind: 'today-share' }
  | { kind: 'lower-retention'; retention: number; days: number }
  | { kind: 'study-all' };
export interface CatchUpResult { postponed: number; keptToday: number; undoLabel: string }

// ───────────────────────── browse / search ─────────────────────────
export type BrowseMode = 'cards' | 'notes';
export type BrowseColumn = 'sortField' | 'question' | 'answer' | 'deck' | 'notetype' | 'template' | 'tags' | 'due' | 'interval' | 'ease' | 'difficulty' | 'stability' | 'retrievability' | 'reps' | 'lapses' | 'created' | 'modified' | 'cardCount';
export interface BrowseRequest {
  query: string; mode: BrowseMode; sort: { column: BrowseColumn; descending: boolean };
  offset: number; limit: number;
}
export interface BrowseRow {
  id: CardId | NoteId; cardId: CardId; noteId: NoteId; cells: Partial<Record<BrowseColumn, string>>;
  flag: Flag; suspended: boolean; buried: boolean; isLeech: boolean;
}
export interface BrowseResult { rows: BrowseRow[]; total: number; ids: Array<CardId | NoteId> }
export interface SidebarItem { kind: 'saved' | 'today' | 'flag' | 'deck' | 'notetype' | 'tag'; label: string; query: string; count?: number; children?: SidebarItem[] }

export type BulkAction =
  | { kind: 'changeDeck'; deckId: DeckId }
  | { kind: 'addTags'; tags: string[] } | { kind: 'removeTags'; tags: string[] }
  | { kind: 'suspend' } | { kind: 'unsuspend' } | { kind: 'bury' } | { kind: 'unbury' }
  | { kind: 'flag'; flag: Flag }
  | { kind: 'reposition'; start: number; step: number; randomize: boolean; shift: boolean }
  | { kind: 'setDue'; days: string }           // Anki syntax: '0', '1!', '3-7'
  | { kind: 'forget'; resetCounts: boolean; restorePosition: boolean }
  | { kind: 'delete' }
  | { kind: 'findReplace'; find: string; replace: string; regex: boolean; field: string | null; matchCase: boolean };

// ───────────────────────── stats ─────────────────────────
export interface Insight { id: string; severity: 'info' | 'suggest' | 'warn'; text: string; action?: { label: string; route: string } }
export interface StatsRequest { deckId: DeckId | null; /* null = whole collection */ }
export interface Stats {
  today: { studied: number; seconds: number; again: number; correctPct: number; learned: number; relearned: number };
  forecast: Array<{ day: number; due: number; withNew: number }>;        // next 90 days
  retention: Array<{ label: string; trueRetention: number | null; desired: number; reviews: number }>; // 7d/30d/90d/1y/all
  reviewsPerDay: Array<{ day: CollectionDay; count: number; seconds: number; learn: number; review: number; relearn: number }>; // 365 days
  heatmap: Array<{ date: string; count: number }>;                        // ISO date, 365 days
  intervals: Array<{ bucket: string; count: number }>;
  stability: Array<{ bucket: string; count: number }>;
  difficulty: Array<{ bucket: string; count: number }>;
  hourly: Array<{ hour: number; reviews: number; correctPct: number }>;
  buttons: { learn: number[]; young: number[]; mature: number[] };        // again/hard/good/easy counts
  cardCounts: { new: number; learning: number; young: number; mature: number; suspended: number; buried: number };
  momentum: { daysStudiedLast30: number; currentRun: number; longestRun: number };
  insights: Insight[];
}

// ───────────────────────── import / export ─────────────────────────
export type PackageLayout = 'legacy1' | 'legacy2' | 'latest';
export interface ImportPreview {
  layout: PackageLayout; isCollectionPackage: boolean;
  notes: number; cards: number; decks: string[]; notetypes: string[]; reviews: number; media: number;
  hasScheduling: boolean; fsrsCards: number; sourceCrt: Seconds; warnings: string[];
  /** (appended) notes whose guid is already in this collection, and notes that would be new. */
  notesExisting: number; notesNew: number;
}
export interface ImportOptions {
  /** 'merge' into this collection by note guid; 'replace' = restore-like replacement of the whole collection. */
  mode: 'merge' | 'replace';
  keepScheduling: boolean;
  /** when a note exists with the same guid: 'newer' wins (default), 'keep' ours, 'theirs' */
  conflict: 'newer' | 'keep' | 'theirs';
}
export interface ImportReport {
  notesAdded: number; notesUpdated: number; notesSkipped: number; cardsAdded: number;
  reviewsAdded: number; mediaAdded: number; decksAdded: number; notetypesAdded: number;
  warnings: string[]; durationMs: number;
}
export interface ExportOptions { deckId: DeckId | null; includeScheduling: boolean; includeMedia: boolean }
export interface Progress { phase: string; done: number; total: number }

// ───────────────────────── backups ─────────────────────────
export interface BackupInfo { name: string; at: Seconds; bytes: number; cards: number | null; kind: 'daily' | 'weekly' | 'monthly' | 'manual' | 'pre-restore' }
export interface IntegrityReport { ok: boolean; problems: string[]; repaired: string[] }

// ───────────────────────── optimiser ─────────────────────────
export interface OptimiseResult {
  configId: DeckConfigId; reviews: number; before: { params: number[]; logLoss: number; rmse: number };
  after: { params: number[]; logLoss: number; rmse: number }; adopted: boolean; reason: string;
}
export interface WorkloadForecast { reviewsPerDay: number[]; minutesPerDay: number[]; sentence: string }

// ───────────────────────── collection-wide settings ─────────────────────────
export interface CollectionSettings {
  rolloverHour: number; learnAheadMinutes: number; locale: string;
  theme: 'system' | 'light' | 'dark';
  cardFont: 'serif' | 'sans' | 'system'; cardFontSize: number; cardAlign: 'start' | 'center';
  showKeyboardHints: boolean; reducedMotion: 'system' | 'on' | 'off';
  typedAnswerIgnoreCase: boolean; typedAnswerIgnoreDiacritics: boolean;
}

// ───────────────────────── sync ─────────────────────────
export interface SyncPeer { deviceId: string; lastSeen: Seconds }
export interface SyncStatus {
  /** Sync has been used on this collection. */
  enabled: boolean;
  deviceId: string | null;
  collectionId: string;
  /** This device's sync folder: a path, or a folder name in the browser. */
  folder: string | null;
  lastSync: Seconds | null;
  peers: SyncPeer[];
  /** Ids this collection had before a restore; their files in a sync folder are stale. */
  formerDevices: string[];
}
export interface SyncExportOptions {
  /** For a sync folder: leave out what the listed devices already have, and media files. */
  folder?: boolean;
  peers?: string[];
}
export interface SyncMediaRef { name: string; sha1: string }
export interface SyncReport {
  fromDevice: string; rowsIn: number; conflictsResolved: number; reviewsAdded: number;
  mediaAdded: number; mediaNeeded: SyncMediaRef[];
  /** False when the other device left out things this one has not seen yet; the next sync fills them in. */
  complete: boolean;
}
export interface SyncMediaState { have: SyncMediaRef[]; need: SyncMediaRef[] }
