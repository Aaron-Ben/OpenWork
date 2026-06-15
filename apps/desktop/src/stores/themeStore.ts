import { create } from 'zustand'

export type Theme = 'light' | 'dark' | 'system'

const THEME_STORAGE_KEY = 'anvil-theme'

function readStoredTheme(): Theme {
  if (typeof localStorage === 'undefined') return 'system'
  const stored = localStorage.getItem(THEME_STORAGE_KEY)
  return stored === 'light' || stored === 'dark' || stored === 'system' ? stored : 'system'
}

interface ThemeStoreState {
  /** 当前主题偏好;`system` 表示跟随操作系统配色。 */
  theme: Theme
  setTheme: (theme: Theme) => void
  /** 在 light → dark → system 间循环切换。 */
  cycleTheme: () => void
}

/**
 * 主题偏好(亮/暗/跟随系统),持久化到 localStorage。
 * 把 `.dark` 类实际应用到 <html> 的副作用在 {@link useTheme} hook 里。
 */
export const useThemeStore = create<ThemeStoreState>((set, get) => ({
  theme: readStoredTheme(),
  setTheme: (theme) => {
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem(THEME_STORAGE_KEY, theme)
    }
    set({ theme })
  },
  cycleTheme: () => {
    const next: Record<Theme, Theme> = { light: 'dark', dark: 'system', system: 'light' }
    get().setTheme(next[get().theme])
  },
}))
