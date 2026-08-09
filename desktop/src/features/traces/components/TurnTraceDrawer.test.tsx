import { renderToStaticMarkup } from 'react-dom/server'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { coreCommands } from '@/bridge/commands'
import type { RuntimeTraceSpan, RuntimeTraceSpanPayload, RuntimeTurnTrace } from '@/bridge/compat'
import {
  buildTracePayloadPreview,
  initialTraceSpanId,
  loadTraceForSource,
  loadTracePayloadWhenExpanded,
  MissingTracePayload,
  SpanDetail,
  TraceSummaryHeader,
  TRACE_PAYLOAD_RENDER_LIMIT_CHARS,
  TracePayloadBody,
} from './TurnTraceDrawer'

vi.mock('@/bridge/commands', () => ({
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

  it('shows model quality and diagnostic fields in the record-property grid', () => {
    const markup = renderToStaticMarkup(<SpanDetail span={modelSpan} />)

    expect(markup).toContain('完成原因')
    expect(markup).toContain('工具调用')
    expect(markup).toContain('温度')
    expect(markup).toContain('请求构建耗时')
    expect(markup).toContain('Top P')
    expect(markup).toContain('Provider Request ID')
    expect(markup).toContain('data-trace-properties="true"')
    expect(markup).not.toContain('<details')
    expect(markup).not.toContain('requestBuildMs')
    expect(markup).not.toContain('tool_use')
    expect(markup).not.toContain('Transport 尝试')
  })

  it('renders token composition, payload actions, and record attributes as scan-friendly sections', () => {
    const markup = renderToStaticMarkup(<SpanDetail span={modelSpan} />)

    expect(markup).toContain('data-token-composition="true"')
    expect(markup).toContain('Token 构成')
    expect(markup).toContain('data-trace-payload-grid="true"')
    expect(markup).toContain('data-trace-properties="true"')
    expect(markup).toContain('记录属性')
    expect(markup.match(/data-token-segment=/g)).toHaveLength(4)
  })

  it('does not paint token segments for zero-valued categories', () => {
    const markup = renderToStaticMarkup(<SpanDetail span={{
      ...modelSpan,
      inputTokens: 0,
      outputTokens: 0,
      cachedInputTokens: 0,
      reasoningTokens: 0,
      totalTokens: 0,
    }} />)

    expect(markup).toContain('data-token-composition="true"')
    expect(markup).not.toContain('data-token-segment=')
  })

  it('keeps complete diagnostic values readable without relying on hover titles', () => {
    const markup = renderToStaticMarkup(<SpanDetail span={{
      ...modelSpan,
      providerRequestId: 'request-with-a-very-long-provider-identifier',
    }} />)

    expect(markup).toContain('data-trace-property-value="true"')
    expect(markup).toContain('break-all')
    expect(markup).not.toContain('data-trace-property-value="true" class="mt-1 truncate')
  })

  it('keeps compaction diagnostics in the record-property grid', () => {
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
    expect(markup).toContain('data-trace-properties="true"')
    expect(markup).toContain('尝试次数')
    expect(markup).toContain('压缩触发原因')
    expect(markup).toContain('准备耗时')
    expect(markup).not.toContain('<details')
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

  it('states payload absence without guessing why it is missing', () => {
    const markup = renderToStaticMarkup(<MissingTracePayload />)

    expect(markup).toContain('无正文记录')
    expect(markup).not.toContain('档位')
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
        modelSubmissionCount: 0, toolCallCount: 0, spanCount: 2, totalTokens: 0,
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

describe('TraceSummaryHeader', () => {
  it('renders the title status and five-column trace overview', () => {
    const markup = renderToStaticMarkup(<TraceSummaryHeader
      title="新会话"
      sourceId="turn-1"
      summary={{
        traceId: 'trace-1', turnId: 'turn-1', sessionId: 'session-1', turnSequence: 1,
        status: 'completed', resolvedModelName: 'deepseek-v4-flash', modelCallCount: 6,
        modelSubmissionCount: 6, toolCallCount: 6, spanCount: 12, totalTokens: 141_635,
        startedAt: '2026-07-20T00:00:00.000Z', endedAt: '2026-07-20T00:00:45.120Z',
      }}
      completeness={{
        expectedModelCalls: 6, capturedModelCalls: 6, expectedToolCalls: 6,
        capturedToolCalls: 6, orphanToolSpans: 0, runningSpans: 0,
        outcomeUnknownSpans: 0, state: 'complete',
      }}
      onClose={vi.fn()}
    />)

    expect(markup).toContain('新会话')
    expect(markup).toContain('已完成')
    expect(markup).toContain('data-trace-overview="true"')
    expect(markup).toContain('总耗时')
    expect(markup).toContain('45.12 s')
    expect(markup).toContain('模型调用')
    expect(markup).toContain('工具调用')
    expect(markup).toContain('141,635')
    expect(markup).toContain('12 / 12')
  })

  it('keeps an incomplete trace visibly marked as an alert in the overview', () => {
    const markup = renderToStaticMarkup(<TraceSummaryHeader
      title="新会话"
      sourceId="turn-1"
      summary={{
        traceId: 'trace-1', turnId: 'turn-1', sessionId: 'session-1', turnSequence: 1,
        status: 'failed', resolvedModelName: 'deepseek-v4-flash', modelCallCount: 2,
        modelSubmissionCount: 2, toolCallCount: 3, spanCount: 3, totalTokens: 100,
        startedAt: '2026-07-20T00:00:00.000Z', endedAt: '2026-07-20T00:00:01.000Z',
      }}
      completeness={{
        expectedModelCalls: 2, capturedModelCalls: 2, expectedToolCalls: 3,
        capturedToolCalls: 1, orphanToolSpans: 0, runningSpans: 0,
        outcomeUnknownSpans: 0, state: 'partial',
      }}
      onClose={vi.fn()}
    />)

    expect(markup).toContain('data-trace-completeness="partial"')
    expect(markup).toContain('role="alert"')
    expect(markup).toContain('缺少 2 条')
    expect(markup).toContain('3 / 5')
    expect(markup).toContain('部分缺失')
  })

  it('explains structural incompleteness even when captured equals expected', () => {
    const markup = renderToStaticMarkup(<TraceSummaryHeader
      title="新会话"
      sourceId="turn-1"
      summary={{
        traceId: 'trace-1', turnId: 'turn-1', sessionId: 'session-1', turnSequence: 1,
        status: 'completed', resolvedModelName: 'deepseek-v4-flash', modelCallCount: 2,
        modelSubmissionCount: 2, toolCallCount: 3, spanCount: 5, totalTokens: 100,
        startedAt: '2026-07-20T00:00:00.000Z', endedAt: '2026-07-20T00:00:01.000Z',
      }}
      completeness={{
        expectedModelCalls: 2, capturedModelCalls: 2, expectedToolCalls: 3,
        capturedToolCalls: 3, orphanToolSpans: 1, runningSpans: 0,
        outcomeUnknownSpans: 1, state: 'partial',
      }}
      onClose={vi.fn()}
    />)

    expect(markup).toContain('5 / 5')
    expect(markup).toContain('1 个孤立工具调用')
    expect(markup).toContain('1 个结果未知节点')
    expect(markup).toContain('bg-status-warning-soft')
    expect(markup).toContain('data-trace-completeness-note="true"')
    expect(markup).toContain('max-[640px]:col-span-2')
    expect(markup).not.toContain('data-trace-completeness-note="true" class="mt-0.5 truncate')
  })

  it('uses the danger tier when trace capture is entirely absent', () => {
    const markup = renderToStaticMarkup(<TraceSummaryHeader
      title="新会话"
      sourceId="turn-1"
      summary={null}
      completeness={{
        expectedModelCalls: 2, capturedModelCalls: 0, expectedToolCalls: 3,
        capturedToolCalls: 0, orphanToolSpans: 0, runningSpans: 0,
        outcomeUnknownSpans: 0, state: 'none',
      }}
      onClose={vi.fn()}
    />)

    expect(markup).toContain('data-trace-completeness="none"')
    expect(markup).toContain('未采集')
    expect(markup).toContain('bg-status-danger-soft')
  })
})
