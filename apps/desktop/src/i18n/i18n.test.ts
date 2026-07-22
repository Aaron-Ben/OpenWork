import { describe, expect, it } from 'vitest'

import i18n, { supportedLanguages } from './index'
import { enUS } from './locales/en-US'
import { zhCN } from './locales/zh-CN'
import { zhTW } from './locales/zh-TW'
import { TRACE_ATTRIBUTE_KEYS } from '../features/traces/traceViewModel'

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
    expect(i18n.t('settings.contextWindow.title')).toBe('上下文窗口')
    expect(i18n.t('settings.appearance.system')).toBe('跟随系统')
    expect(i18n.t('activity.title')).toBe('运行记录')
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

  it('localizes every trace attribute and attempt detail in all supported languages', () => {
    for (const language of supportedLanguages) {
      const translate = i18n.getFixedT(language)
      for (const key of TRACE_ATTRIBUTE_KEYS) {
        const path = `activity.traceFields.${key}`
        expect(translate(path)).not.toBe(path)
      }
      for (const key of ['errorCode', 'providerRequestId', 'retryDelayMs'] as const) {
        const path = `activity.traceFields.${key}`
        expect(translate(path)).not.toBe(path)
      }
      for (const value of [
        'started', 'failed', 'succeeded', 'tool_use', 'stream_decode',
        'semantic_output_emitted', 'allow', 'policy', 'enabled', 'true', 'false',
      ] as const) {
        const path = `activity.traceValues.${value}`
        expect(translate(path)).not.toBe(path)
      }
      expect(translate('activity.transportAttemptSummary', {
        index: 2,
        status: translate('activity.traceValues.succeeded'),
        duration: '80 ms',
      })).not.toContain('activity.transportAttemptSummary')
    }

    expect(i18n.getFixedT('zh-CN')('activity.traceFields.requestBuildMs')).toBe('请求构建耗时')
    expect(i18n.getFixedT('zh-TW')('activity.traceFields.requestBuildMs')).toBe('請求建置耗時')
    expect(i18n.getFixedT('en-US')('activity.traceFields.requestBuildMs')).toBe('Request build time')
  })
})
