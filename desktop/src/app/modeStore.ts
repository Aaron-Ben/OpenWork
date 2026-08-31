import { create } from 'zustand'

export type AppMode = 'workbench' | 'collab'

const MODE_STORAGE_KEY = 'openwork-mode'

export function normalizeStoredMode(value: string | null): AppMode {
  return value === 'collab' || value === 'workbench' ? value : 'workbench'
}

function readStoredMode(): AppMode {
  if (typeof localStorage === 'undefined') return 'workbench'
  return normalizeStoredMode(localStorage.getItem(MODE_STORAGE_KEY))
}

interface ModeStoreState {
  mode: AppMode
  setMode: (mode: AppMode) => void
  toggleMode: () => void
}

export const useModeStore = create<ModeStoreState>((set, get) => ({
  mode: readStoredMode(),
  setMode: (mode) => {
    if (typeof localStorage !== 'undefined') localStorage.setItem(MODE_STORAGE_KEY, mode)
    set({ mode })
  },
  toggleMode: () => get().setMode(get().mode === 'workbench' ? 'collab' : 'workbench'),
}))
