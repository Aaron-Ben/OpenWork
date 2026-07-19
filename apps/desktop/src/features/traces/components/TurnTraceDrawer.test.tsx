import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import type { RuntimeTraceSpan } from '../../../bridge/compat'
import { SpanDetail } from './TurnTraceDrawer'

const modelSpan: RuntimeTraceSpan = {
  id: 'model-1', turnId: 'turn-1', parentSpanId: null, sequence: 1, kind: 'model_call', name: 'model.call',
  status: 'succeeded', modelId: 'model-1', resolvedModelName: 'deepseek-v4-flash', providerRequestId: 'request-1',
  providerCallId: null, requestedToolName: null, resolvedToolName: null, attemptCount: 1, inputTokens: 10,
  outputTokens: 20, cachedInputTokens: 1, reasoningTokens: 4, totalTokens: 30, permissionWaitMs: null,
  startedAt: '2026-07-20T00:00:00.000Z', endedAt: '2026-07-20T00:00:02.000Z', errorCode: null,
  errorMessage: null,
  attributes: {
    schemaVersion: 1,
    requestBuildMs: 7,
    finishReason: 'tool_use',
    resultPersisted: true,
    attempts: [{
      index: 1,
      status: 'succeeded',
      durationMs: 80,
      errorPhase: 'stream_decode',
      deliveryState: 'semantic_output_emitted',
      retryDelayMs: 25,
    }],
  },
}

describe('SpanDetail', () => {
  it('renders trace field names and enum values in the active language', () => {
    const markup = renderToStaticMarkup(<SpanDetail span={modelSpan} />)

    expect(markup).toContain('请求构建耗时')
    expect(markup).toContain('完成原因')
    expect(markup).toContain('工具调用')
    expect(markup).toContain('第 1 次尝试')
    expect(markup).toContain('错误阶段：流式解码')
    expect(markup).toContain('交付状态：已产生语义输出')
    expect(markup).toContain('重试等待：25 ms')
    expect(markup).not.toContain('requestBuildMs')
    expect(markup).not.toContain('tool_use')
    expect(markup).not.toContain('retry:')
  })
})
