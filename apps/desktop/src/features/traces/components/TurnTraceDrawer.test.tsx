import { renderToStaticMarkup } from 'react-dom/server'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { coreCommands } from '../../../bridge/commands'
import type { RuntimeTraceSpan, RuntimeTraceSpanPayload, RuntimeTurnTrace } from '../../../bridge/compat'
import {
  buildTracePayloadPreview,
  initialTraceSpanId,
  loadTraceForSource,
  loadTracePayloadWhenExpanded,
  MissingTracePayload,
  SpanDetail,
  TRACE_PAYLOAD_RENDER_LIMIT_CHARS,
  TracePayloadBody,
} from './TurnTraceDrawer'

vi.mock('../../../bridge/commands', () => ({
  coreCommands: {
    getTrace: vi.fn(),
    getTraceById: vi.fn(),
    getSpanPayload: vi.fn(),
  },
}))

const modelSpan: RuntimeTraceSpan = {
  id: 'model-1', traceId: 'turn-1', sessionId: 'session-1', turnId: 'turn-1', parentSpanId: null, kind: 'model_call', name: 'model.call',
  status: 'succeeded', modelId: 'model-1', resolvedModelName: 'deepseek-v4-flash', providerRequestId: 'request-1',
  providerCallId: null, requestedToolName: null, resolvedToolName: null, attemptCount: 1, inputTokens: 10,
  outputTokens: 20, cachedInputTokens: 1, reasoningTokens: 4, totalTokens: 30, responseMessageId: null, permissionWaitMs: null,
  startedAt: '2026-07-20T00:00:00.000Z', endedAt: '2026-07-20T00:00:02.000Z', errorCode: null,
  errorMessage: null,
  attributes: {
    schemaVersion: 1,
    requestBuildMs: 7,
    finishReason: 'tool_use',
    temperature: 0.2,
    topP: 0.9,
    resultPersisted: true,
  },
}

describe('SpanDetail', () => {
  beforeEach(() => vi.clearAllMocks())

  it('shows model quality fields before the collapsed detail section', () => {
    const markup = renderToStaticMarkup(<SpanDetail span={modelSpan} />)
    const detailsIndex = markup.indexOf('<details')

    expect(markup).toContain('完成原因')
    expect(markup).toContain('工具调用')
    expect(markup).toContain('温度')
    expect(detailsIndex).toBeGreaterThan(0)
    expect(markup.indexOf('温度')).toBeLessThan(detailsIndex)
    expect(markup.indexOf('请求构建耗时')).toBeGreaterThan(detailsIndex)
    expect(markup.indexOf('Top P')).toBeGreaterThan(detailsIndex)
    expect(markup.indexOf('Provider Request ID')).toBeGreaterThan(detailsIndex)
    expect(markup).not.toContain('requestBuildMs')
    expect(markup).not.toContain('tool_use')
    expect(markup).not.toContain('Transport 尝试')
  })

  it('keeps the compaction attempt count in the primary section', () => {
    const markup = renderToStaticMarkup(<SpanDetail span={{
      ...modelSpan,
      id: 'compaction-1',
      kind: 'compaction',
      name: 'session.compact',
      resolvedModelName: null,
      modelId: null,
      inputTokens: null,
      outputTokens: null,
      cachedInputTokens: null,
      reasoningTokens: null,
      totalTokens: null,
      attemptCount: 2,
      attributes: {
        trigger: 'manual',
        conversationTokensBefore: 1000,
        conversationTokensAfter: 200,
        reclaimedConversationTokens: 800,
        prepareMs: 3,
      },
    }} />)
    const detailsIndex = markup.indexOf('<details')

    expect(markup.indexOf('尝试次数')).toBeLessThan(detailsIndex)
    expect(markup.indexOf('压缩触发原因')).toBeLessThan(detailsIndex)
    expect(markup.indexOf('准备耗时')).toBeGreaterThan(detailsIndex)
  })

  it('renders a successful model response as a conversation link', () => {
    const markup = renderToStaticMarkup(<SpanDetail
      span={{ ...modelSpan, responseMessageId: 'message-1' }}
      onOpenMessage={vi.fn()}
    />)

    expect(markup).toContain('在聊天记录中查看响应')
    expect(markup).toContain('data-payload-slot="response"')
  })

  it('keeps a large payload out of the DOM preview until explicitly expanded', () => {
    const body = { source: 'x'.repeat(TRACE_PAYLOAD_RENDER_LIMIT_CHARS * 2) }
    const preview = buildTracePayloadPreview(body)
    const payload: RuntimeTraceSpanPayload = {
      spanId: 'model-1',
      slot: 'request',
      body,
      byteSize: 65_536,
      truncated: true,
      originalByteSize: 1_048_576,
      redactedCount: 19,
    }
    const markup = renderToStaticMarkup(<TracePayloadBody payload={payload} />)

    expect(preview.limited).toBe(true)
    expect(preview.preview.length).toBeLessThanOrEqual(TRACE_PAYLOAD_RENDER_LIMIT_CHARS)
    expect(markup).toContain('已截断，原始 1.0 MiB')
    expect(markup).toContain('展开全部')
    expect(markup).not.toContain('redacted')
    expect(markup.length).toBeLessThan(TRACE_PAYLOAD_RENDER_LIMIT_CHARS + 5_000)
  })

  it('states payload absence as a fact and only describes the current policy', () => {
    const markup = renderToStaticMarkup(<MissingTracePayload policy="off" />)

    expect(markup).toContain('无正文记录')
    expect(markup).toContain('当前内容记录不是')
    expect(markup).not.toContain('当时')
  })

  it('does not request a payload until its slot is expanded', async () => {
    const fetchPayload = vi.fn().mockResolvedValue(null)

    expect(await loadTracePayloadWhenExpanded(false, 'span-1', 'request', fetchPayload)).toBeUndefined()
    expect(fetchPayload).not.toHaveBeenCalled()
    expect(await loadTracePayloadWhenExpanded(true, 'span-1', 'request', fetchPayload)).toBeNull()
    expect(fetchPayload).toHaveBeenCalledWith('span-1', 'request')
  })

  it('opens a manual /compact trace by trace id and then loads its summary payload', async () => {
    const rootSpan: RuntimeTraceSpan = {
      ...modelSpan,
      id: 'compact-root',
      traceId: 'trace-manual',
      turnId: null,
      kind: 'compaction',
      name: 'session.compact',
      modelId: null,
      responseMessageId: null,
    }
    const summarySpan = {
      ...modelSpan,
      id: 'summary-model',
      traceId: 'trace-manual',
      turnId: null,
      parentSpanId: 'compact-root',
    }
    const trace: RuntimeTurnTrace = {
      summary: {
        traceId: 'trace-manual', turnId: null, sessionId: 'session-1', turnSequence: null,
        status: 'completed', resolvedModelName: 'deepseek-v4-flash', modelCallCount: 0,
        modelSubmissionCount: 0, toolCallCount: 0, spanCount: 2,
        startedAt: '2026-07-20T00:00:00.000Z', endedAt: '2026-07-20T00:00:02.000Z',
      },
      spans: [rootSpan, summarySpan],
      completeness: {
        expectedModelCalls: 0, capturedModelCalls: 0, expectedToolCalls: 0,
        capturedToolCalls: 0, orphanToolSpans: 0, runningSpans: 0,
        outcomeUnknownSpans: 0, state: 'complete',
      },
    }
    vi.mocked(coreCommands.getTraceById).mockResolvedValue(trace)
    const payload = { spanId: summarySpan.id, slot: 'response', body: { text: 'summary body' }, byteSize: 23, truncated: false, originalByteSize: null, redactedCount: 0 } as const
    vi.mocked(coreCommands.getSpanPayload).mockResolvedValue(payload)

    const loaded = await loadTraceForSource({ kind: 'trace', traceId: 'trace-manual' })
    const selectedSpanId = initialTraceSpanId(loaded, 'trace')
    const loadedPayload = await loadTracePayloadWhenExpanded(
      true,
      selectedSpanId ?? '',
      'response',
    )

    expect(coreCommands.getTraceById).toHaveBeenCalledWith('trace-manual')
    expect(coreCommands.getTrace).not.toHaveBeenCalled()
    expect(selectedSpanId).toBe('summary-model')
    expect(coreCommands.getSpanPayload).toHaveBeenCalledWith('summary-model', 'response')
    expect(loadedPayload?.body).toEqual({ text: 'summary body' })
  })
})
