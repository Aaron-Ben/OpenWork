import type { useTranslation } from 'react-i18next'

import { formatElapsedClock } from './agentPresentation'

type Translate = ReturnType<typeof useTranslation>['t']

/**
 * 顶栏副标题：「主控 + 3 个子智能体 · 12 步 · 01:48」。
 * 还没有数据的段直接省略 —— 显示「0 步」会让人以为已经跑过但什么都没做。
 */
export function formatSessionHeadline(
  subAgentCount: number,
  steps: number,
  elapsedMs: number | null,
  t: Translate,
): string {
  const segments = [
    subAgentCount > 0
      ? t('chat.header.multiAgent', { count: subAgentCount })
      : t('chat.header.singleAgent'),
  ]
  if (steps > 0) segments.push(t('chat.header.steps', { count: steps }))
  if (elapsedMs != null) segments.push(formatElapsedClock(elapsedMs))
  return segments.join(' · ')
}
