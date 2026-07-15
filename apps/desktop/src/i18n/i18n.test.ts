import { describe, expect, it } from 'vitest'

import i18n, { supportedLanguages } from './index'
import { enUS } from './locales/en-US'
import { zhCN } from './locales/zh-CN'
import { zhTW } from './locales/zh-TW'

function keyPaths(value: object, prefix = ''): string[] {
  return Object.entries(value).flatMap(([key, child]) => {
    const path = prefix ? `${prefix}.${key}` : key
    return typeof child === 'object' && child !== null ? keyPaths(child, path) : [path]
  })
}

describe('i18n', () => {
  it('supports Simplified Chinese, Traditional Chinese, and English', () => {
    expect(supportedLanguages).toEqual(['zh-CN', 'zh-TW', 'en-US'])
    expect(i18n.resolvedLanguage).toBe('zh-CN')
  })

  it('loads the shell and settings translations', () => {
    expect(i18n.t('sidebar.newSession')).toBe('创建会话')
    expect(i18n.t('settings.models.title')).toBe('模型配置')
    expect(i18n.t('settings.appearance.system')).toBe('跟随系统')
    expect(i18n.t('settings.trace.title')).toBe('运行追踪')
  })

  it('provides complete navigation labels in every supported language', () => {
    expect(i18n.getFixedT('zh-TW')('sidebar.newSession')).toBe('建立對話')
    expect(i18n.getFixedT('en-US')('sidebar.newSession')).toBe('New conversation')
    expect(i18n.getFixedT('en-US')('settings.appearance.language')).toBe('Language')
  })

  it('keeps all locale resources on the same key structure', () => {
    const expected = keyPaths(zhCN).sort()
    expect(keyPaths(zhTW).sort()).toEqual(expected)
    expect(keyPaths(enUS).sort()).toEqual(expected)
  })
})
