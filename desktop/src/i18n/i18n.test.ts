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

  it('loads the settings translations', () => {
    expect(i18n.t('settings.general.title')).toBe('通用')
    expect(i18n.t('settings.appearance.system')).toBe('跟随系统')
    expect(i18n.getFixedT('en-US')('settings.appearance.language')).toBe('Language')
  })

  it('keeps all locale resources on the same key structure', () => {
    const expected = keyPaths(zhCN).sort()
    expect(keyPaths(zhTW).sort()).toEqual(expected)
    expect(keyPaths(enUS).sort()).toEqual(expected)
  })

  it('localizes the P0 collaboration surface in all supported languages', () => {
    const paths = [
      'collab.nav.rooms',
      'collab.nav.agents',
      'collab.nav.boards',
      'collab.nav.observability',
      'collab.nav.settings',
      'collab.rooms.title',
      'collab.rooms.messagePlaceholder',
      'collab.rooms.createGroup',
      'collab.rooms.manageMembers',
      'collab.rooms.workingOne',
      'collab.rooms.workingTwo',
      'collab.rooms.workingMany',
      'collab.agents.title',
      'collab.agents.mainModel',
      'collab.agents.persona',
      'collab.boards.title',
      'collab.observability.title',
      'collab.observability.timeline',
    ] as const
    for (const language of supportedLanguages) {
      const translate = i18n.getFixedT(language)
      for (const path of paths) expect(translate(path)).not.toBe(path)
    }
  })
})
