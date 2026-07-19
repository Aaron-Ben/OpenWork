import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import type { RuntimeTraceSpan } from '../../../bridge/compat'
import { TraceTimeline } from './TraceTimeline'

const model: RuntimeTraceSpan = {
  id: 'model-1', turnId: 'turn-1', parentSpanId: null, sequence: 1, kind: 'model_call', name: 'model',
  status: 'succeeded', modelId: 'model-1', resolvedModelName: 'deepseek-v4-flash', providerRequestId: 'req-1',
  providerCallId: null, requestedToolName: null, resolvedToolName: null, attemptCount: 2, inputTokens: 10,
  outputTokens: 20, cachedInputTokens: 1, reasoningTokens: 4, totalTokens: 30,
  permissionWaitMs: null, startedAt: '2026-07-18T00:00:00.000Z',
  endedAt: '2026-07-18T00:00:02.000Z', errorCode: null, errorMessage: null, attributes: {},
}

const tool: RuntimeTraceSpan = {
  ...model, id: 'tool-1', parentSpanId: 'model-1', sequence: 2, kind: 'tool_call', name: 'read', modelId: null,
  resolvedModelName: null, providerRequestId: null, providerCallId: 'call-1', requestedToolName: 'read',
  resolvedToolName: 'read_file', attemptCount: null, inputTokens: null, outputTokens: null,
  cachedInputTokens: null, reasoningTokens: null, totalTokens: null, permissionWaitMs: 120,
  startedAt: '2026-07-18T00:00:00.500Z',
  endedAt: '2026-07-18T00:00:01.000Z',
}

describe('TraceTimeline', () => {
  it('renders the model/tool hierarchy and proportional waterfall bars', () => {
    const markup = renderToStaticMarkup(
      <TraceTimeline spans={[model, tool]} selectedSpanId="tool-1" onSelect={vi.fn()} />,
    )

    expect(markup).toContain('deepseek-v4-flash')
    expect(markup).toContain('read_file')
    expect(markup).toContain('data-trace-waterfall="true"')
    expect(markup).toContain('data-span-id="model-1"')
    expect(markup).toContain('data-span-id="tool-1"')
    expect(markup).toContain('margin-left:25%')
    expect(markup).toContain('width:25%')
  })
})
