/** App-wide UI state (zustand). Collection data lives behind the API, not here. */
import { create } from 'zustand';
import type { CollectionSettings, Progress } from '@core/types';
import { applyTheme, readStoredTheme, resolveTheme, storeTheme, systemPrefersDark, type ResolvedTheme, type ThemePref } from './theme';

export interface BusyOp { op: string; progress: Progress; at: number }

interface UiState {
  themePref: ThemePref;
  theme: ResolvedTheme;
  setThemePref: (pref: ThemePref) => void;
  syncSystemTheme: () => void;

  addSheet: { open: boolean; deckId?: number | null; noteId?: number | null };
  openAdd: (opts?: { deckId?: number | null; noteId?: number | null }) => void;
  closeAdd: () => void;

  helpOpen: boolean;
  setHelpOpen: (open: boolean) => void;

  busy: Record<string, BusyOp>;
  setBusy: (op: string, progress: Progress | null) => void;

  settings: CollectionSettings | null;
  setSettings: (s: CollectionSettings) => void;

  /** preset ids being optimised from the preset editor (their own toast replaces the global one) */
  manualOptimise: number[];
  setManualOptimise: (configId: number, on: boolean) => void;
}

const storage = typeof window !== 'undefined' ? window.localStorage : null;
const initialPref = readStoredTheme(storage);

export const useUi = create<UiState>((set, get) => ({
  themePref: initialPref,
  theme: resolveTheme(initialPref, systemPrefersDark()),
  setThemePref: (pref) => {
    storeTheme(storage, pref);
    const theme = resolveTheme(pref, systemPrefersDark());
    applyTheme(theme, true);
    set({ themePref: pref, theme });
  },
  syncSystemTheme: () => {
    const theme = resolveTheme(get().themePref, systemPrefersDark());
    applyTheme(theme, true);
    set({ theme });
  },

  addSheet: { open: false },
  openAdd: (opts) => set({ addSheet: { open: true, ...opts } }),
  closeAdd: () => set({ addSheet: { open: false } }),

  helpOpen: false,
  setHelpOpen: (helpOpen) => set({ helpOpen }),

  busy: {},
  setBusy: (op, progress) => {
    const busy = { ...get().busy };
    if (!progress || (progress.total > 0 && progress.done >= progress.total)) delete busy[op];
    else busy[op] = { op, progress, at: Date.now() };
    set({ busy });
  },

  settings: null,
  setSettings: (settings) => {
    applySettingsToDocument(settings);
    set({ settings });
  },

  manualOptimise: [],
  setManualOptimise: (configId, on) => {
    const rest = get().manualOptimise.filter((id) => id !== configId);
    set({ manualOptimise: on ? [...rest, configId] : rest });
  },
}));

const CARD_FONTS: Record<CollectionSettings['cardFont'], string> = {
  serif: 'var(--font-card)',
  sans: 'var(--font-card-sans)',
  system: 'var(--font-card-system)',
};

/** Mirrors collection settings onto <html> (card font/size, motion, keyboard hints). */
export function applySettingsToDocument(s: CollectionSettings): void {
  const html = document.documentElement;
  html.style.setProperty('--loci-card-font', CARD_FONTS[s.cardFont] ?? CARD_FONTS.serif);
  // 22px is the designed default (20px on phones, from tokens.css); anything else scales both.
  if (s.cardFontSize > 0 && s.cardFontSize !== 22) {
    html.style.setProperty('--card-user-size', `${s.cardFontSize / 16}rem`);
    html.dataset.cardSize = 'custom';
  } else {
    html.style.removeProperty('--card-user-size');
    delete html.dataset.cardSize;
  }
  html.dataset.cardAlign = s.cardAlign;
  if (s.reducedMotion === 'on') html.dataset.motion = 'off';
  else if (s.reducedMotion === 'off') html.dataset.motion = 'on';
  else delete html.dataset.motion;
  html.dataset.kbdHints = s.showKeyboardHints ? 'on' : 'off';
}
