/** Hands a picked/dropped file from Welcome (or anywhere) to the Import view. */
import { create } from 'zustand';

interface ImportFileState {
  file: File | null;
  mode: 'merge' | 'replace' | null;
  set: (file: File | null, mode?: 'merge' | 'replace' | null) => void;
  take: () => { file: File | null; mode: 'merge' | 'replace' | null };
}

export const useImportFile = create<ImportFileState>((set, get) => ({
  file: null,
  mode: null,
  set: (file, mode = null) => set({ file, mode }),
  take: () => { const { file, mode } = get(); set({ file: null, mode: null }); return { file, mode }; },
}));

/** Anki packages, plus Loci's own full backup (.zip, restored by replacing the collection). */
export const ACCEPTED_EXTENSIONS = ['.apkg', '.colpkg', '.zip'];
export function isPackageName(name: string): boolean {
  const n = name.toLowerCase();
  return ACCEPTED_EXTENSIONS.some((e) => n.endsWith(e));
}
