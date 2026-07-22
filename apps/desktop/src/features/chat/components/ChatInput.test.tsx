import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import { ChatInput } from './ChatInput'

const baseProps = {
  model: 'deepseek-chat',
  modelOptions: [
    {
      modelId: 'deepseek-chat',
      displayName: 'DeepSeek Chat',
      modelTier: 'plus' as const,
      enabled: true,
    },
    {
      modelId: 'deepseek-reasoner',
      displayName: 'DeepSeek Reasoner',
      modelTier: 'pro' as const,
      enabled: true,
    },
  ],
  value: 'Explain this repository',
  isSending: false,
  onValueChange: vi.fn(),
  onModelChange: vi.fn(),
  onSubmit: vi.fn(),
}

describe('ChatInput toolbar', () => {
  it('renders the compact approval, model, and send controls', () => {
    const markup = renderToStaticMarkup(
      <ChatInput
        {...baseProps}
        contextUsage={{ usedTokens: 66_000, totalTokens: 258_000, estimated: false }}
      />,
    )

    expect(markup).toContain('询问审批')
    expect(markup).not.toContain('由我审批')
    expect(markup).toContain('aria-label="选择模型"')
    expect(markup).toContain('DeepSeek Chat · Plus')
    expect(markup).toContain('aria-label="发送"')
    expect(markup).toContain('data-context-usage-ring="true"')
    expect(markup).toContain('role="tooltip"')
    expect(markup).toContain('上下文窗口')
    expect(markup).toContain('26% 已用（剩余 74%）')
    expect(markup).toContain('66k / 258k Tokens 已用')
    expect(markup).toContain('w-[clamp(104px,20vw,200px)]')
    expect(markup).not.toContain('w-[clamp(110px,28vw,260px)]')
    expect(markup).not.toContain('overflow-hidden rounded-[18px]')
    expect(markup).not.toContain('麦克风')
    expect(markup).not.toContain('Demo Provider')
    expect(markup).not.toContain('运行')
    expect(markup).toContain('min-h-[80px]')
    expect(markup).toContain('rows="2"')
    expect(markup).not.toContain('min-h-[84px]')
    expect(markup).toContain('min-h-12')
    expect(markup).toContain('size-9')
    expect(markup).not.toContain('min-h-14')
    expect(markup).not.toContain('size-10')
    expect(markup).toContain('data-slot="button"')
    expect(markup).toContain('data-motion-component="chat-input"')
  })

  it('keeps the context affordance available when usage has not been measured', () => {
    const markup = renderToStaticMarkup(<ChatInput {...baseProps} />)

    expect(markup).toContain('data-context-usage-progress="0"')
    expect(markup).toContain('暂无上下文用量')
  })

  it('replaces the send affordance with a compact stop control while streaming', () => {
    const markup = renderToStaticMarkup(<ChatInput {...baseProps} isSending />)

    expect(markup).toContain('aria-label="停止生成"')
    expect(markup).not.toContain('Streaming')
  })

  it('renders pending approval content immediately above the input form', () => {
    const markup = renderToStaticMarkup(
      <ChatInput
        {...baseProps}
        topContent={<div data-testid="pending-approval">需要审批</div>}
      />,
    )

    const approvalPosition = markup.indexOf('data-testid="pending-approval"')
    const inputPosition = markup.indexOf('data-motion-component="chat-input"')

    expect(approvalPosition).toBeGreaterThan(-1)
    expect(inputPosition).toBeGreaterThan(approvalPosition)
  })
})
