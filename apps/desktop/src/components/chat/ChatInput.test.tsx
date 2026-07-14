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
    const markup = renderToStaticMarkup(<ChatInput {...baseProps} />)

    expect(markup).toContain('询问审批')
    expect(markup).not.toContain('由我审批')
    expect(markup).toContain('aria-label="选择模型"')
    expect(markup).toContain('DeepSeek Chat · Plus')
    expect(markup).toContain('aria-label="发送"')
    expect(markup).not.toContain('Context Window')
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
