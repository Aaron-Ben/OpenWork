import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import type { TraceSpanDetailView } from '../../type/trace'
import { getTraceDetailTabs, TraceSpanDetailPanel } from './TraceSpanDetail'

const toolDetail: TraceSpanDetailView = {
  span: {
    traceId: 'turn-1', spanId: 'tool-1', parentSpanId: 'step-1', spanKind: 'tool_run',
    spanName: 'tool.run', status: 'succeeded', sessionId: 'session-1', turnId: 'turn-1',
    stepId: 'step-1', toolRunId: 'tool-run-1', startedAt: 1_000, endedAt: 14_000,
    durationMs: 13_000, attributes: { toolName: 'bash' }, errorType: null,
    errorCode: null, errorMessage: null,
  },
  detail: {
    kind: 'tool_run',
    data: {
      toolName: 'bash', providerToolCallId: 'call-1', input: { command: 'cargo test' },
      observation: {
        status: 'succeeded', content: [{ type: 'text', text: 'tests passed' }], error: null,
      },
      approvalRequired: true, requestedAt: 1_000, executionStartedAt: 13_980,
      requestToEndMs: 13_000, approvalWaitMs: 12_965, executionMs: 20,
    },
  },
}

const modelDetail = {
  span: {
    ...toolDetail.span,
    spanId: 'model-1',
    spanKind: 'model_attempt',
    spanName: 'model.attempt',
    toolRunId: null,
  },
  detail: {
    kind: 'model_attempt',
    data: {
      providerId: 'provider-1', model: 'glm-5.1', finishReason: 'stop', rawFinishReason: null,
      responseId: 'response-1', providerRequestId: 'request-1', firstOutputMs: 120,
      usage: { inputTokens: 90, outputTokens: 10, totalTokens: 100, cachedInputTokens: 20, reasoningTokens: 5 },
      requestSummary: {
        version: '1', messageCount: 4, messageTextChars: 320, systemPromptChars: 120,
        toolDefinitionCount: 2, toolNames: ['read', 'bash'], temperature: 0.2,
        maxOutputTokens: 2048, thinkingMode: 'enabled',
      },
      messages: [],
    },
  },
} as TraceSpanDetailView

const source = [
  '# deadlock.py',
  'import threading',
  '',
  'if __name__ == "__main__":',
  '    print("start")',
].join('\n')

const writeDetail = {
  ...toolDetail,
  detail: {
    ...toolDetail.detail,
    data: {
      ...toolDetail.detail.data,
      toolName: 'write',
      input: { path: '/workspace/deadlock.py', content: source },
      observation: null,
    },
  },
} as TraceSpanDetailView

describe('TraceSpanDetailPanel', () => {
  it('renders a LangSmith-style overview, input/output, and metadata navigation', () => {
    const markup = renderToStaticMarkup(<TraceSpanDetailPanel value={toolDetail} />)

    expect(getTraceDetailTabs(toolDetail)).toEqual(['overview', 'input_output', 'metadata'])
    expect(markup).toContain('data-trace-detail-tab-button="overview"')
    expect(markup).toContain('data-trace-detail-tab-button="input_output"')
    expect(markup).toContain('data-trace-detail-tab-button="metadata"')
    expect(markup).toContain('aria-selected="true"')
    expect(markup).toContain('概览')
    expect(markup).toContain('输入与输出')
    expect(markup).toContain('元数据')
    expect(markup).toContain('bg-status-success-soft')
    expect(markup).toContain('text-status-success-ink')
  })

  it('uses a theme-aware danger surface for failed Span errors', () => {
    const failed = {
      ...toolDetail,
      span: {
        ...toolDetail.span,
        status: 'failed',
        errorCode: 'tool_failed',
        errorMessage: 'command failed',
      },
    } as TraceSpanDetailView
    const markup = renderToStaticMarkup(<TraceSpanDetailPanel value={failed} />)

    expect(markup).toContain('border-status-danger-border')
    expect(markup).toContain('bg-status-danger-soft')
    expect(markup).toContain('text-status-danger-ink')
    expect(markup).not.toContain('bg-red-50')
  })

  it('renders Tool request, approval and execution durations as separate overview facts', () => {
    const markup = renderToStaticMarkup(<TraceSpanDetailPanel value={toolDetail} />)

    expect(markup).toContain('请求到结束')
    expect(markup).toContain('审批等待')
    expect(markup).toContain('实际执行')
    expect(markup).toContain('12.965 秒')
    expect(markup).toContain('20 ms')
  })

  it('renders Tool input and Observation together in the input/output view', () => {
    const markup = renderToStaticMarkup(
      <TraceSpanDetailPanel value={toolDetail} initialTab="input_output" />,
    )

    expect(markup).toContain('data-trace-detail-panel="input_output"')
    expect(markup).toContain('cargo test')
    expect(markup).toContain('tests passed')
  })

  it('renders Turn outcome and the content-free model request shape', () => {
    const markup = renderToStaticMarkup(<TraceSpanDetailPanel value={modelDetail} />)

    expect(markup).toContain('请求结构摘要')
    expect(markup).toContain('消息数')
    expect(markup).toContain('320')
    expect(markup).toContain('read, bash')
    expect(markup).not.toContain('prompt text')
  })

  it('renders a string content field as source instead of escaped JSON', () => {
    const markup = renderToStaticMarkup(
      <TraceSpanDetailPanel value={writeDetail} initialTab="input_output" />,
    )

    expect(markup).toContain('data-tool-input-content="true"')
    expect(markup).toContain('/workspace/deadlock.py')
    expect(markup).toContain('if __name__ == &quot;__main__&quot;:')
    expect(markup).not.toContain('&quot;content&quot;')
    expect(markup).not.toContain('deadlock.py\\nimport threading')
  })

  it('keeps stable IDs, timestamps, and captured attributes in metadata', () => {
    const markup = renderToStaticMarkup(
      <TraceSpanDetailPanel value={toolDetail} initialTab="metadata" />,
    )

    expect(markup).toContain('data-trace-detail-panel="metadata"')
    expect(markup).toContain('Trace ID')
    expect(markup).toContain('Span ID')
    expect(markup).toContain('tool-1')
    expect(markup).toContain('Tool Run ID')
    expect(markup).toContain('采集属性')
    expect(markup).toContain('&quot;toolName&quot;: &quot;bash&quot;')
  })
})
