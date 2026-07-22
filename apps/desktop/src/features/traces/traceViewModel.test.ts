import { describe, expect, it } from 'vitest'

import type { RuntimeTraceSpan, RuntimeTraceSummary } from '../../bridge/compat'
import {
  buildTraceAttributeRows,
  buildTraceAttributeSections,
  readTraceAttempts,
  buildTraceListItems,
  buildTraceTree,
  buildWaterfallRows,
  filterTraceListItems,
  shouldPollTrace,
} from './traceViewModel'

const summaries: RuntimeTraceSummary[] = [
  {
    turnId: 'turn-1',
    sessionId: 'session-1',
    turnSequence: 2,
    status: 'completed',
    resolvedModelName: 'deepseek-v4-flash',
    modelCallCount: 1,
    toolCallCount: 1,
    spanCount: 2,
    startedAt: '2026-07-18T00:00:00.000Z',
    endedAt: '2026-07-18T00:00:02.000Z',
  },
]

const model: RuntimeTraceSpan = {
  id: 'span-model', turnId: 'turn-1', parentSpanId: null, sequence: 1, kind: 'model_call',
  name: 'model', status: 'succeeded', modelId: 'model-1', resolvedModelName: 'deepseek-v4-flash',
  providerRequestId: 'request-1', providerCallId: null, requestedToolName: null,
  resolvedToolName: null, attemptCount: 1, inputTokens: 10, outputTokens: 20,
  cachedInputTokens: 2, reasoningTokens: 4, totalTokens: 30, permissionWaitMs: null,
  startedAt: '2026-07-18T00:00:00.000Z',
  endedAt: '2026-07-18T00:00:02.000Z', errorCode: null, errorMessage: null, attributes: {},
}

const tool: RuntimeTraceSpan = {
  ...model,
  id: 'span-tool',
  parentSpanId: 'span-model',
  sequence: 2,
  kind: 'tool_call',
  name: 'read',
  modelId: null,
  resolvedModelName: null,
  providerRequestId: null,
  providerCallId: 'call-1',
  requestedToolName: 'read',
  resolvedToolName: 'read_file',
  inputTokens: null,
  outputTokens: null,
  cachedInputTokens: null,
  reasoningTokens: null,
  totalTokens: null,
  permissionWaitMs: 120,
  startedAt: '2026-07-18T00:00:00.500Z',
  endedAt: '2026-07-18T00:00:01.000Z',
}

describe('traceViewModel', () => {
  it('adds user-recognizable session context and searches it', () => {
    const items = buildTraceListItems(summaries, {
      'session-1': { title: '修复登录', workingDirectory: '/repo/openwork' },
    })

    expect(items[0]).toMatchObject({ title: '修复登录', workingDirectory: '/repo/openwork', durationMs: 2000 })
    expect(filterTraceListItems(items, '登录', 'all')).toEqual(items)
    expect(filterTraceListItems(items, 'openwork', 'completed')).toEqual(items)
  })

  it('builds a model-to-tool tree and keeps orphan tool calls visible', () => {
    const orphan = { ...tool, id: 'orphan', parentSpanId: 'missing', sequence: 3 }
    const tree = buildTraceTree([model, tool, orphan])

    expect(tree.models).toHaveLength(1)
    expect(tree.models[0].children.map((item) => item.id)).toEqual(['span-tool'])
    expect(tree.orphans.map((item) => item.id)).toEqual(['orphan'])
  })

  it('maps actual span timing into proportional waterfall rows', () => {
    const rows = buildWaterfallRows([model, tool], Date.parse('2026-07-18T00:00:03.000Z'))

    expect(rows[0]).toMatchObject({ span: model, leftPercent: 0, widthPercent: 100, durationMs: 2000 })
    expect(rows[1].leftPercent).toBe(25)
    expect(rows[1].widthPercent).toBe(25)
  })

  it('keeps polling while a summary or loaded span is still running', () => {
    expect(shouldPollTrace('running', [])).toBe(true)
    expect(shouldPollTrace('completed', [{ ...tool, endedAt: null }])).toBe(true)
    expect(shouldPollTrace('completed', [tool])).toBe(false)
  })

  it('exposes versioned P0/P1 fields without requiring raw payloads', () => {
    const span = {
      ...model,
      attributes: {
        schemaVersion: 1,
        requestBuildMs: 7,
        ttftMs: 20,
        finishReason: 'stop',
        requestMessageCount: 4,
        requestContentBytes: 512,
        requestEstimatedSystemContextTokens: 20,
        requestEstimatedConversationTokens: 80,
        requestEstimatedToolSurfaceTokens: 10,
        requestEstimatedInputTokens: 110,
        attempts: [{ index: 1, status: 'succeeded', durationMs: 80 }],
      },
    }
    const rows = buildTraceAttributeRows(span)

    expect(rows).toEqual(expect.arrayContaining([
      { key: 'schemaVersion', value: '1' },
      { key: 'requestBuildMs', value: '7 ms' },
      { key: 'finishReason', value: 'stop' },
      { key: 'requestMessageCount', value: '4' },
      { key: 'requestContentBytes', value: '512 B' },
      { key: 'requestEstimatedSystemContextTokens', value: '20' },
      { key: 'requestEstimatedInputTokens', value: '110' },
      { key: 'attempts', value: '1' },
    ]))
    const sections = buildTraceAttributeSections(span)
    expect(sections.p0.map((row) => row.key)).toContain('requestBuildMs')
    expect(sections.p1.map((row) => row.key)).toContain('requestMessageCount')
    expect(readTraceAttempts({
      ...model,
      attributes: { attempts: [{ index: 1, status: 'succeeded', durationMs: 80 }] },
    })).toEqual([{ index: 1, status: 'succeeded', durationMs: 80 }])
  })
})
