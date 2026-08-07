import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import type { ContentBlock } from '@/types/parts'
import { AssistantMessage } from './AssistantMessage'
import { UserMessage } from './UserMessage'

function copyableTextOf(markup: string): string | null {
  // 复制按钮不把内容写进 DOM，所以这里只能验证按钮在不在。
  return markup.includes('data-copy-button="true"') ? 'present' : null
}

describe('消息级复制入口', () => {
  it('用户消息有复制按钮，且平时隐藏', () => {
    const parts: ContentBlock[] = [{ type: 'text', text: 'Wdf，为什么不进行创建' }]

    const markup = renderToStaticMarkup(<UserMessage parts={parts} />)

    expect(copyableTextOf(markup)).toBe('present')
    // 用 opacity 而不是条件渲染：条件渲染会在悬停瞬间改变布局，消息会跳。
    expect(markup).toContain('opacity-0')
    expect(markup).toContain('group-hover:opacity-100')
    // 键盘用户也要够得着。
    expect(markup).toContain('group-focus-within:opacity-100')
  })

  it('空的用户消息不渲染，自然也没有复制按钮', () => {
    const markup = renderToStaticMarkup(<UserMessage parts={[{ type: 'text', text: '   ' }]} />)

    expect(markup).toBe('')
  })

  it('助手消息有复制按钮', () => {
    const parts: ContentBlock[] = [
      { type: 'thinking', thinking: '先想一下' },
      { type: 'text', text: '这是回答' },
    ]

    const markup = renderToStaticMarkup(<AssistantMessage parts={parts} />)

    expect(copyableTextOf(markup)).toBe('present')
  })

  it('流式输出期间复制入口占位但不可见', () => {
    const parts: ContentBlock[] = [{ type: 'text', text: '正在输出的半句' }]

    const streaming = renderToStaticMarkup(<AssistantMessage parts={parts} isStreaming />)
    const settled = renderToStaticMarkup(<AssistantMessage parts={parts} />)

    // 按钮钉在正文末行右侧，流式期间用 invisible 占位：布局不跳，也复制不了半句话。
    expect(copyableTextOf(streaming)).toBe('present')
    expect(streaming).toContain('invisible')
    expect(copyableTextOf(settled)).toBe('present')
    expect(settled).not.toContain('invisible')
  })

  it('只有思考和工具、没有正文时不给复制入口', () => {
    const parts: ContentBlock[] = [
      { type: 'thinking', thinking: '只是想了想' },
      {
        type: 'tool_call',
        id: 'call-1',
        name: 'bash',
        input: '{"command":"ls"}',
        state: 'finished',
      },
    ]

    const markup = renderToStaticMarkup(<AssistantMessage parts={parts} />)

    // thinking 是草稿，工具有自己的复制入口，都不算"这条回答"。
    expect(copyableTextOf(markup)).toBeNull()
  })
})
