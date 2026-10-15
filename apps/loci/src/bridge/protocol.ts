/**
 * The RPC protocol between the UI and the Rust engine (served by `loci-core::rpc`): the method
 * table, what each call may have changed, and which calls write.
 * A Tauri transport (`tauri.ts`) and a local-HTTP transport for development and tests (`http.ts`)
 * both speak it.
 */
import type { CollectionApi, CollectionEvent } from '@core/api';
import type { Progress } from '@core/types';

/** Methods whose first argument is a byte payload (sent as a staged raw body). */
export const BYTES_IN = new Set<string>(['previewImport', 'importPackage', 'mergeSyncBundle']);
/** Methods whose second argument is a byte payload. */
export const BYTES_IN_SECOND = new Set<string>(['addMedia', 'addSyncMedia']);
/** Methods that return bytes (or null). */
export const BYTES_OUT = new Set<string>(['getMedia', 'exportPackage', 'exportFullBackup', 'exportSyncBundle']);

export interface SerializedError { code: string; message: string; detail?: unknown }
export interface ProgressCallback { (p: Progress): void }

/** Methods the worker serves: the whole CollectionApi plus the leave-time backup. */
export type RpcMethod = keyof CollectionApi | 'backupOnLeave' | 'autoBackup';

/** Every method the worker serves (a Record over RpcMethod, so the compiler checks it is complete). */
const METHODS: Record<RpcMethod, true> = {
  addMedia: true,
  addNote: true,
  allTags: true,
  answerCard: true,
  applyCatchUp: true,
  backlogReport: true,
  backupNow: true,
  browse: true,
  bulk: true,
  buryCards: true,
  checkIntegrity: true,
  checkMedia: true,
  checkTemplate: true,
  close: true,
  createDeck: true,
  deckTree: true,
  deleteDeck: true,
  deleteMedia: true,
  deleteNotes: true,
  deleteNotetype: true,
  deletePreset: true,
  deleteSavedSearch: true,
  endStudy: true,
  evaluate: true,
  explainCard: true,
  exportFullBackup: true,
  exportPackage: true,
  extendTodayLimits: true,
  findDuplicates: true,
  forecastWorkload: true,
  getCard: true,
  getCardsOfNote: true,
  getDeck: true,
  getMedia: true,
  getNote: true,
  getNotetype: true,
  getPreset: true,
  getSettings: true,
  importPackage: true,
  insights: true,
  installSampleDeck: true,
  isEmpty: true,
  listBackups: true,
  listNotetypes: true,
  listPresets: true,
  maintenance: true,
  nextCard: true,
  open: true,
  optimise: true,
  previewImport: true,
  previewNote: true,
  renameDeck: true,
  renameTag: true,
  renderCard: true,
  resetCollection: true,
  resolveLeech: true,
  restoreBackup: true,
  saveNotetype: true,
  savePreset: true,
  saveSearch: true,
  searchIds: true,
  setDeckCollapsed: true,
  setDeckDescription: true,
  setDeckPreset: true,
  setFlag: true,
  setSettings: true,
  sidebar: true,
  startCustomStudy: true,
  startStudy: true,
  stats: true,
  suspendCards: true,
  today: true,
  unburyDeck: true,
  undo: true,
  undoLabel: true,
  updateNote: true,
  backupOnLeave: true,
  autoBackup: true,
  syncStatus: true,
  setSyncFolder: true,
  exportSyncBundle: true,
  mergeSyncBundle: true,
  syncMediaState: true,
  addSyncMedia: true,
};
export const isRpcMethod = (m: unknown): m is RpcMethod => typeof m === 'string' && Object.prototype.hasOwnProperty.call(METHODS, m);

type Change = Extract<CollectionEvent, { type: 'changed' }>['what'][number];
const ALL: Change[] = ['decks', 'notes', 'cards', 'notetypes', 'presets', 'settings', 'tags', 'backups'];

/** What each successful call may have changed (drives the UI's refetching). */
export const CHANGES: Partial<Record<RpcMethod, Change[]>> = {
  setSettings: ['settings'],
  createDeck: ['decks'], renameDeck: ['decks'], deleteDeck: ['decks', 'cards', 'notes'], setDeckCollapsed: ['decks'],
  setDeckPreset: ['decks', 'presets'], setDeckDescription: ['decks'],
  savePreset: ['presets', 'decks'], deletePreset: ['presets', 'decks'],
  saveNotetype: ['notetypes', 'notes', 'cards', 'decks'], deleteNotetype: ['notetypes'],
  addNote: ['notes', 'cards', 'decks', 'tags'], updateNote: ['notes', 'cards', 'decks', 'tags'], deleteNotes: ['notes', 'cards', 'decks', 'tags'],
  renameTag: ['tags', 'notes'],
  answerCard: ['cards', 'decks'], suspendCards: ['cards', 'decks'], buryCards: ['cards', 'decks'], setFlag: ['cards'],
  unburyDeck: ['cards', 'decks'], extendTodayLimits: ['decks'], resolveLeech: ['cards', 'notes', 'decks', 'tags'],
  undo: ALL,
  applyCatchUp: ['cards', 'decks', 'presets'], bulk: ['cards', 'notes', 'decks', 'tags'],
  saveSearch: ['settings'], deleteSavedSearch: ['settings'],
  optimise: ['presets', 'cards'],
  importPackage: ALL, installSampleDeck: ['decks', 'notes', 'cards', 'tags'],
  backupNow: ['backups'], backupOnLeave: ['backups'], autoBackup: ['backups'], restoreBackup: ALL, resetCollection: ALL, checkIntegrity: ALL,
  mergeSyncBundle: ALL, setSyncFolder: ['settings'],
};

/** Calls that write (the client asks for persistent storage before the first one). */
export const isWrite = (m: RpcMethod): boolean => m in CHANGES || m === 'addMedia' || m === 'deleteMedia' || m === 'maintenance' || m === 'addSyncMedia';
