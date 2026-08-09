import { describe, expect, it } from 'vitest'

import type { ChatItem } from '@/types/chat'
import type { ContentBlock } from '@/types/parts'
import { transcriptGap, transcriptGapClass } from './transcriptSpacing'

const TURN = 'turn-1'

function item(id: string, overrides: Partial<ChatItem> = {}): ChatItem {
  return {
    id,
    turnId: TURN,
    role: 'assistant',
    parts: [],
    ...overrides,
  }
}

function toolCall(id: string): ContentBlock {
  return { type: 'tool_call', id, name: 'bash', input: '{}', state: 'finished' }
}

function toolResult(id: string): ContentBlock {
  return { type: 'tool_result', id, name: 'bash', output: [{ type: 'text', text: 'ok' }], state: 'success' }
}

function text(value: string): ContentBlock {
  return { type: 'text', text: value }
}

function thinking(value: string): ContentBlock {
  return { type: 'thinking', thinking: value }
}

const toolsOnly = (id: string, callId: string) =>
  item(id, { parts: [toolCall(callId), toolResult(callId)] })

describe('transcriptGap', () => {
  it('第一条消息不带上间距', () => {
    expect(transcriptGap(undefined, toolsOnly('a', 'c1'))).toBe('none')
  })

  it('两条纯工具消息之间贴合，与同一条消息内的工具行一致', () => {
    expect(transcriptGap(toolsOnly('a', 'c1'), toolsOnly('b', 'c2'))).toBe('tight')
  })

  it('上一条以正文收尾、下一条是工具行时用块间距', () => {
    const prose = item('a', { parts: [thinking('想一下'), text('开始写')] })
    expect(transcriptGap(prose, toolsOnly('b', 'c2'))).toBe('block')
  })

  it('上一条以正文和工具行混排时，仍按工具行收尾贴合', () => {
    const mixed = item('a', { parts: [text('先跑一下'), toolCall('c1'), toolResult('c1')] })
    expect(transcriptGap(mixed, toolsOnly('b', 'c2'))).toBe('tight')
  })

  it('计划卡作为独立块，其后的工具行不贴合', () => {
    const planned = item('a', {
      parts: [toolCall('c1'), toolResult('c1')],
      plan: {
        explanation: null,
        steps: [],
        updateCount: 1,
        startedAt: null,
        updatedAt: '2026-08-09T00:00:00Z',
      },
    })
    expect(transcriptGap(planned, toolsOnly('b', 'c2'))).toBe('block')
  })

  it('下一条以正文开头时回到段落间距', () => {
    const prose = item('b', { parts: [text('测试都过了')] })
    expect(transcriptGap(toolsOnly('a', 'c1'), prose)).toBe('section')
  })

  it('下一条只有 model 抬头时不贴合', () => {
    const labelled = item('b', { parts: [toolCall('c2')], model: 'claude-opus-5' })
    expect(transcriptGap(toolsOnly('a', 'c1'), labelled)).toBe('section')
  })

  it('用户消息两侧一律用段落间距', () => {
    const user = item('u', { role: 'user', parts: [text('继续')] })
    expect(transcriptGap(toolsOnly('a', 'c1'), user)).toBe('section')
    expect(transcriptGap(user, toolsOnly('b', 'c2'))).toBe('section')
  })

  it('跨 Turn 不贴合', () => {
    const next = item('b', { turnId: 'turn-2', parts: [toolCall('c2')] })
    expect(transcriptGap(toolsOnly('a', 'c1'), next)).toBe('section')
  })

  it('缺少 turnId 时不贴合', () => {
    const orphan = item('a', { turnId: undefined, parts: [toolCall('c1')] })
    expect(transcriptGap(orphan, toolsOnly('b', 'c2'))).toBe('section')
  })

  it('文件汇总卡自成一块', () => {
    const summary = item('b', {
      parts: [toolCall('c2'), toolResult('c2')],
      fileChangePresentation: 'summary',
    })
    expect(transcriptGap(toolsOnly('a', 'c1'), summary)).toBe('section')
  })

  it('流式空消息按正文起头处理', () => {
    const streaming = item('b', { parts: [], isStreaming: true })
    expect(transcriptGap(toolsOnly('a', 'c1'), streaming)).toBe('section')
  })

  it('tool 角色的残留结果消息参与贴合', () => {
    const orphanResult = item('b', { role: 'tool', parts: [toolResult('c2')] })
    expect(transcriptGap(toolsOnly('a', 'c1'), orphanResult)).toBe('tight')
  })
})

describe('transcriptGapClass', () => {
  it('三档数值与消息内部的间距一一对应', () => {
    expect(transcriptGapClass('none')).toBe('')
    expect(transcriptGapClass('tight')).toBe('mt-0.5')
    expect(transcriptGapClass('block')).toBe('mt-1.5')
    expect(transcriptGapClass('section')).toBe('mt-4')
  })
})
