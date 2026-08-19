import { afterEach, describe, expect, it, vi } from 'vitest'

import { normalizeStoredMode, useModeStore } from './modeStore'

describe('modeStore', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('accepts only durable shell modes', () => {
    expect(normalizeStoredMode('collab')).toBe('collab')
    expect(normalizeStoredMode('workbench')).toBe('workbench')
    expect(normalizeStoredMode('obsolete')).toBe('workbench')
    expect(normalizeStoredMode(null)).toBe('workbench')
  })

  it('persists mode changes without zustand middleware', () => {
    const setItem = vi.fn()
    vi.stubGlobal('localStorage', { getItem: vi.fn(), setItem })
    useModeStore.getState().setMode('collab')
    expect(useModeStore.getState().mode).toBe('collab')
    expect(setItem).toHaveBeenCalledWith('openwork-mode', 'collab')
  })
})
