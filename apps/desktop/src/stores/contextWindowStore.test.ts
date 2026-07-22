import { afterEach, describe, expect, it, vi } from 'vitest'

import {
  CONTEXT_WINDOW_STORAGE_KEY,
  DEFAULT_CONTEXT_WINDOW_TOKENS,
  parseContextWindowTokens,
  useContextWindowStore,
} from './contextWindowStore'

afterEach(() => {
  vi.unstubAllGlobals()
  useContextWindowStore.setState({ contextWindowTokens: DEFAULT_CONTEXT_WINDOW_TOKENS })
})

describe('contextWindowStore', () => {
  it('accepts only positive safe integer token counts', () => {
    expect(parseContextWindowTokens('258000')).toBe(258_000)
    expect(parseContextWindowTokens(128_000)).toBe(128_000)
    expect(parseContextWindowTokens('')).toBeNull()
    expect(parseContextWindowTokens('1.5')).toBeNull()
    expect(parseContextWindowTokens('0')).toBeNull()
    expect(parseContextWindowTokens('-1')).toBeNull()
  })

  it('persists a valid application-level context window', () => {
    const setItem = vi.fn()
    vi.stubGlobal('localStorage', { setItem })

    useContextWindowStore.getState().setContextWindowTokens(512_000)

    expect(useContextWindowStore.getState().contextWindowTokens).toBe(512_000)
    expect(setItem).toHaveBeenCalledWith(CONTEXT_WINDOW_STORAGE_KEY, '512000')
  })
})
