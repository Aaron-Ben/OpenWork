import { create } from 'zustand'

export const DEFAULT_CONTEXT_WINDOW_TOKENS = 258_000
export const CONTEXT_WINDOW_STORAGE_KEY = 'openwork-context-window-tokens'

interface ContextWindowStoreState {
  contextWindowTokens: number
  setContextWindowTokens: (tokens: number) => void
}

export function parseContextWindowTokens(value: unknown): number | null {
  const parsed = typeof value === 'number'
    ? value
    : typeof value === 'string' && value.trim()
      ? Number(value)
      : Number.NaN
  return Number.isSafeInteger(parsed) && parsed > 0 ? parsed : null
}

function readStoredContextWindowTokens(): number {
  if (typeof localStorage === 'undefined') return DEFAULT_CONTEXT_WINDOW_TOKENS
  return parseContextWindowTokens(localStorage.getItem(CONTEXT_WINDOW_STORAGE_KEY))
    ?? DEFAULT_CONTEXT_WINDOW_TOKENS
}

export const useContextWindowStore = create<ContextWindowStoreState>((set) => ({
  contextWindowTokens: readStoredContextWindowTokens(),
  setContextWindowTokens: (tokens) => {
    const parsed = parseContextWindowTokens(tokens)
    if (parsed === null) return
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem(CONTEXT_WINDOW_STORAGE_KEY, String(parsed))
    }
    set({ contextWindowTokens: parsed })
  },
}))
