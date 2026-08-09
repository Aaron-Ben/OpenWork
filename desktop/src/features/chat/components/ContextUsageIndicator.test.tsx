import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import { ContextUsageIndicator, ContextUsagePanel } from './ContextUsageIndicator'

const usage = { usedTokens: 600_000, totalTokens: 1_000_000, estimated: true }
const breakdown = { messagesTokens: 561_000, systemPromptTokens: 5_000, systemToolsTokens: 3_000 }

describe('ContextUsageIndicator', () => {
  it('renders only the ring button until clicked', () => {
    const markup = renderToStaticMarkup(
      <ContextUsageIndicator usage={usage} breakdown={breakdown} onInspect={vi.fn()} />,
    )

    expect(markup).toContain('data-context-usage-ring="true"')
    expect(markup).toContain('data-context-usage-progress="60"')
    expect(markup).toContain('aria-haspopup="dialog"')
    expect(markup).toContain('aria-expanded="false"')
    expect(markup).toContain('class="text-clay transition-[stroke-dashoffset] duration-300"')
    expect(markup).toContain('font-mono text-[11px] tabular-nums leading-none text-ink-faint')
    expect(markup).not.toContain('data-context-usage-panel')
  })
})

describe('ContextUsagePanel', () => {
  it('shows the summary bar without the breakdown while collapsed', () => {
    const markup = renderToStaticMarkup(
      <ContextUsagePanel usage={usage} breakdown={breakdown} onShowDetails={vi.fn()} />,
    )

    expect(markup).toContain('data-context-usage-panel="true"')
    expect(markup).toContain('上下文窗口')
    expect(markup).toContain('600k / 1m (60%)')
    expect(markup).toContain('aria-expanded="false"')
    expect(markup).not.toContain('data-context-usage-breakdown')
    expect(markup).not.toContain('详情')
  })

  it('lists only messages, system prompt, and system tools when expanded', () => {
    const markup = renderToStaticMarkup(
      <ContextUsagePanel usage={usage} breakdown={breakdown} onShowDetails={vi.fn()} defaultExpanded />,
    )

    expect(markup).toContain('data-context-usage-breakdown="true"')
    expect(markup).toContain('aria-expanded="true"')
    expect(markup).toContain('消息')
    expect(markup).toContain('系统提示')
    expect(markup).toContain('系统工具')
    expect(markup).toContain('561k')
    expect(markup).toContain('56%')
    expect(markup).toContain('0.5%')
    expect(markup).toContain('详情')
  })

  it('keeps the details entry but flags the missing breakdown', () => {
    const markup = renderToStaticMarkup(
      <ContextUsagePanel usage={usage} breakdown={null} onShowDetails={vi.fn()} defaultExpanded />,
    )

    expect(markup).toContain('下次上下文刷新后显示分类。')
    expect(markup).toContain('详情')
  })

  it('hides the details entry when inspection is unavailable', () => {
    const markup = renderToStaticMarkup(
      <ContextUsagePanel usage={null} breakdown={null} defaultExpanded />,
    )

    expect(markup).toContain('暂无上下文用量')
    expect(markup).not.toContain('详情')
  })
})
