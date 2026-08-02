import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import type { RuntimeTraceSpanPayload } from '../../../bridge/compat'
import {
  parseTracePayloadMessages,
  TracePayloadConversation,
  type TracePayloadMessage,
} from './TracePayloadConversation'
import { TracePayloadBody, TracePayloadModal } from './TurnTraceDrawer'

vi.mock('../../../bridge/commands', () => ({
  coreCommands: {
    getTrace: vi.fn(),
    getTraceById: vi.fn(),
    getSpanPayload: vi.fn(),
  },
}))

const messages: TracePayloadMessage[] = [
  { role: 'system', content: [{ type: 'text', text: 'You are OpenWork.' }] },
  { role: 'user', content: [{ type: 'text', text: 'fix the bug' }] },
  {
    role: 'assistant',
    content: [
      { type: 'thinking', thinking: 'plan first' },
      { type: 'tool_call', id: 'call-1', name: 'read_file', input: '{"path":"src/a.ts"}', state: 'finished' },
    ],
  },
  {
    role: 'tool',
    content: [{
      type: 'tool_result',
      id: 'call-1',
      name: 'read_file',
      output: [{ type: 'text', text: 'file contents here' }],
      state: 'success',
    }],
  },
  {
    role: 'user',
    content: [{
      type: 'data',
      source: { source_type: 'base64', data: 'QUJDREVGR0hJSktMTU4=', media_type: 'image/png' },
      name: null,
    }],
  },
]

describe('parseTracePayloadMessages', () => {
  it('parses the assembled request message array and rejects other shapes', () => {
    expect(parseTracePayloadMessages(messages)).toHaveLength(5)
    expect(parseTracePayloadMessages({ messages })).toBeNull()
    expect(parseTracePayloadMessages([])).toBeNull()
    expect(parseTracePayloadMessages('[]')).toBeNull()
    expect(parseTracePayloadMessages([{ role: 'user' }])).toBeNull()
    expect(parseTracePayloadMessages([{ role: 1, content: [] }])).toBeNull()
  })
})

describe('TracePayloadConversation', () => {
  it('renders role-labeled message cards with block-aware content', () => {
    const markup = renderToStaticMarkup(<TracePayloadConversation messages={messages} />)

    for (const role of ['系统', '用户', '助手', '工具']) {
      expect(markup).toContain(role)
    }
    expect(markup).toContain('data-payload-role="system"')
    expect(markup).toContain('You are OpenWork.')
    expect(markup).toContain('思考')
    expect(markup).toContain('read_file')
    expect(markup).toContain('file contents here')
    // base64 数据永远不直接进 DOM
    expect(markup).not.toContain('QUJDREVGR0hJSktMTU4=')
    expect(markup).toContain('base64 · 20 字符')
  })

  it('collapses long text blocks behind an explicit expand', () => {
    const long = [{ role: 'user', content: [{ type: 'text' as const, text: 'x'.repeat(1200) }] }]
    const markup = renderToStaticMarkup(<TracePayloadConversation messages={long} />)

    expect(markup).toContain('展开全部')
    expect(markup).not.toContain('x'.repeat(601))
  })
})

describe('TracePayloadModal', () => {
  const payload: RuntimeTraceSpanPayload = {
    spanId: 'span-1',
    slot: 'request',
    body: messages,
    byteSize: 1000,
    truncated: false,
    originalByteSize: null,
    redactedCount: 0,
  }

  it('renders the loaded payload in a dialog with slot and span context', () => {
    const markup = renderToStaticMarkup(
      <TracePayloadModal
        slot="request"
        spanTitle="deepseek-v4-flash"
        state={{ status: 'loaded', payload }}
        onClose={vi.fn()}
      />,
    )

    expect(markup).toContain('role="dialog"')
    expect(markup).toContain('data-payload-modal="request"')
    expect(markup).toContain('请求')
    expect(markup).toContain('deepseek-v4-flash')
    expect(markup).toContain('data-payload-message="0"')
    expect(markup).toContain('关闭正文查看窗')
  })

  it('shows loading, missing, and error states instead of the payload', () => {
    const loading = renderToStaticMarkup(
      <TracePayloadModal slot="request" spanTitle="m" state={{ status: 'loading' }} onClose={vi.fn()} />,
    )
    const missing = renderToStaticMarkup(
      <TracePayloadModal slot="request" spanTitle="m" state={{ status: 'loaded', payload: null }} onClose={vi.fn()} />,
    )
    const failed = renderToStaticMarkup(
      <TracePayloadModal slot="request" spanTitle="m" state={{ status: 'error', message: 'boom' }} onClose={vi.fn()} />,
    )

    expect(loading).toContain('正在加载正文')
    expect(missing).toContain('无正文记录')
    expect(failed).toContain('boom')
  })
})

describe('TracePayloadBody conversation view', () => {
  it('prefers the conversation view for request payloads and offers a JSON fallback toggle', () => {
    const payload: RuntimeTraceSpanPayload = {
      spanId: 'span-1',
      slot: 'request',
      body: messages,
      byteSize: 1000,
      truncated: false,
      originalByteSize: null,
      redactedCount: 0,
    }
    const markup = renderToStaticMarkup(<TracePayloadBody payload={payload} />)

    expect(markup).toContain('data-payload-message="0"')
    expect(markup).toContain('data-payload-view-toggle="true"')
    expect(markup).toContain('消息')
    expect(markup).toContain('JSON')
    // 默认不再渲染整段 JSON
    expect(markup).not.toContain('<pre')
  })

  it('falls back to the JSON view when the body is not a message array', () => {
    const payload: RuntimeTraceSpanPayload = {
      spanId: 'span-1',
      slot: 'request',
      body: { unexpected: true },
      byteSize: 20,
      truncated: false,
      originalByteSize: null,
      redactedCount: 0,
    }
    const markup = renderToStaticMarkup(<TracePayloadBody payload={payload} />)

    expect(markup).not.toContain('data-payload-view-toggle')
    expect(markup).toContain('<pre')
  })
})
