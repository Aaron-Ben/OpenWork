import { describe, expect, it } from 'vitest'

import i18n from '@/i18n'
import { formatSessionHeadline } from './sessionHeadline'

const t = i18n.getFixedT('zh-CN')

describe('formatSessionHeadline', () => {
  it('names the shape of the run before anything else', () => {
    expect(formatSessionHeadline(0, 4, 21_000, t)).toBe('单智能体 · 4 步 · 00:21')
    expect(formatSessionHeadline(3, 12, 108_000, t)).toBe('主控 + 3 个子智能体 · 12 步 · 01:48')
  })

  it('omits a segment rather than printing a zero for it', () => {
    expect(formatSessionHeadline(0, 0, null, t)).toBe('单智能体')
    expect(formatSessionHeadline(0, 0, 5_000, t)).toBe('单智能体 · 00:05')
    expect(formatSessionHeadline(2, 7, null, t)).toBe('主控 + 2 个子智能体 · 7 步')
  })

  it('reads in every supported language', () => {
    expect(formatSessionHeadline(3, 12, 108_000, i18n.getFixedT('en-US')))
      .toBe('Orchestrator + 3 sub-agents · 12 steps · 01:48')
    expect(formatSessionHeadline(0, 4, 21_000, i18n.getFixedT('zh-TW')))
      .toBe('單智能體 · 4 步 · 00:21')
  })
})
