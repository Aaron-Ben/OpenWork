import { useEffect } from 'react'

import { useThemeStore, type Theme } from '../stores/themeStore'

/** `system` 模式下查询操作系统当前是否为暗色。 */
function resolveDark(theme: Theme): boolean {
  if (theme === 'dark') return true
  if (theme === 'light') return false
  return typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: dark)').matches
}

/**
 * 把当前主题应用到 <html>(加/去 `.dark` 类)。
 * `system` 模式下监听系统配色变化自动跟随。在应用根调用一次即可。
 */
export function useTheme(): void {
  const theme = useThemeStore((state) => state.theme)

  useEffect(() => {
    const root = document.documentElement
    const apply = (): void => {
      root.classList.toggle('dark', resolveDark(theme))
    }

    apply()

    if (theme !== 'system' || typeof window === 'undefined') return
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    mq.addEventListener('change', apply)
    return () => mq.removeEventListener('change', apply)
  }, [theme])
}
