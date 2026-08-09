import { setTheme as setNativeTheme } from '@tauri-apps/api/app'
import { isTauri } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { syncNativeTheme } from './useTheme'

vi.mock('@tauri-apps/api/app', () => ({ setTheme: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ isTauri: vi.fn() }))

describe('syncNativeTheme', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(isTauri).mockReturnValue(true)
    vi.mocked(setNativeTheme).mockResolvedValue()
  })

  it.each([
    ['light', 'light'],
    ['dark', 'dark'],
    ['system', null],
  ] as const)('keeps the native window aligned with the %s theme', async (theme, expected) => {
    await syncNativeTheme(theme)

    expect(setNativeTheme).toHaveBeenCalledWith(expected)
  })

  it('does not invoke the native API in a browser preview', async () => {
    vi.mocked(isTauri).mockReturnValue(false)

    await syncNativeTheme('light')

    expect(setNativeTheme).not.toHaveBeenCalled()
  })
})
