import { setTheme as setNativeTheme } from '@tauri-apps/api/app'
import { isTauri } from '@tauri-apps/api/core'
import { useEffect } from 'react'

import { useThemeStore, type Theme } from '@/app/themeStore'

/** `system` 模式下查询操作系统当前是否为暗色。 */
function resolveDark(theme: Theme): boolean {
  if (theme === 'dark') return true
  if (theme === 'light') return false
  return typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: dark)').matches
}

/** 网页与原生窗口必须使用同一外观，否则 macOS 失焦按钮会与标题栏背景失去对比。 */
export async function syncNativeTheme(theme: Theme): Promise<void> {
  if (!isTauri()) return
  await setNativeTheme(theme === 'system' ? null : theme)
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
      void syncNativeTheme(theme).catch((error) => {
        console.error('Failed to synchronize the native window theme:', error)
      })
    }

    apply()

    if (theme !== 'system' || typeof window === 'undefined') return
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    mq.addEventListener('change', apply)
    return () => mq.removeEventListener('change', apply)
  }, [theme])
}
