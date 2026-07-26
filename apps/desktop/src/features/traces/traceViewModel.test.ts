import { describe, expect, it } from 'vitest'

import type { RuntimeTraceSpan, RuntimeTraceSummary } from '../../bridge/compat'
import {
  buildCompactionHistory,
  buildTraceAttributeRows,
  buildTraceAttributeSections,
  buildTraceListItems,
  buildTraceTree,
  buildWaterfallRows,
  filterTraceListItems,
  shouldPollTrace,
  TRACE_ATTRIBUTE_KEYS,
  TRACE_ATTRIBUTE_PLACEMENT,
} from './traceViewModel'

const summaries: RuntimeTraceSummary[] = [
  {
    traceId: 'turn-1',
    turnId: 'turn-1',
    sessionId: 'session-1',
    turnSequence: 2,
    status: 'completed',
    resolvedModelName: 'deepseek-v4-flash',
    modelCallCount: 1,
    modelSubmissionCount: 1,
    toolCallCount: 1,
    spanCount: 2,
    startedAt: '2026-07-18T00:00:00.000Z',
    endedAt: '2026-07-18T00:00:02.000Z',
  },
]

const model: RuntimeTraceSpan = {
  id: 'span-model', traceId: 'turn-1', sessionId: 'session-1', turnId: 'turn-1', parentSpanId: null, kind: 'model_call',
  name: 'model', status: 'succeeded', modelId: 'model-1', resolvedModelName: 'deepseek-v4-flash',
  providerRequestId: 'request-1', providerCallId: null, requestedToolName: null,
  resolvedToolName: null, attemptCount: 1, inputTokens: 10, outputTokens: 20,
  cachedInputTokens: 2, reasoningTokens: 4, totalTokens: 30, responseMessageId: null, permissionWaitMs: null,
  startedAt: '2026-07-18T00:00:00.000Z',
  endedAt: '2026-07-18T00:00:02.000Z', errorCode: null, errorMessage: null, attributes: {},
}

const tool: RuntimeTraceSpan = {
  ...model,
  id: 'span-tool',
  parentSpanId: 'span-model',
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

const compaction: RuntimeTraceSpan = {
  ...model,
  id: 'span-compaction',
  kind: 'compaction',
  name: 'session.compact',
  attemptCount: 1,
  startedAt: '2026-07-18T00:00:00.250Z',
  endedAt: '2026-07-18T00:00:01.000Z',
  attributes: {
    schemaVersion: 1,
    trigger: 'threshold',
    summaryMs: 240,
    summaryChars: 1800,
  },
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
    const orphan = {
      ...tool,
      id: 'orphan',
      parentSpanId: 'missing',
      startedAt: '2026-07-18T00:00:03.000Z',
    }
    const tree = buildTraceTree([model, tool, orphan])

    expect(tree.models).toHaveLength(1)
    expect(tree.models[0].children.map((item) => item.id)).toEqual(['span-tool'])
    expect(tree.orphans.map((item) => item.id)).toEqual(['orphan'])
  })

  it('keeps compaction as an ordered root operation instead of a model call', () => {
    const laterModel = {
      ...model,
      id: 'span-model-2',
      startedAt: '2026-07-18T00:00:03.000Z',
      endedAt: '2026-07-18T00:00:04.000Z',
    }
    const summaryModel = {
      ...model,
      id: 'span-summary-model',
      parentSpanId: 'span-compaction',
      startedAt: '2026-07-18T00:00:00.500Z',
      endedAt: '2026-07-18T00:00:00.900Z',
      status: 'degenerate',
      attributes: { summaryRetryDelayMs: 3000 },
    }
    const tree = buildTraceTree([laterModel, summaryModel, compaction])

    expect(tree.models).toHaveLength(2)
    expect(tree.roots.map((node) => node.span.id)).toEqual(['span-compaction', 'span-model-2'])
    expect(tree.roots[0].children.map((span) => span.id)).toEqual(['span-summary-model'])
    expect(tree.roots[0].children[0].status).toBe('degenerate')
    expect(buildTraceAttributeRows(summaryModel)).toEqual(expect.arrayContaining([
      { key: 'summaryRetryDelayMs', value: '3000 ms' },
    ]))
  })

  it('reports what a compaction reclaimed without storing an attempt rollup', () => {
    const measured: RuntimeTraceSpan = {
      ...compaction,
      attributes: {
        ...compaction.attributes,
        conversationTokensBefore: 12_000,
        conversationTokensAfter: 900,
        reclaimedConversationTokens: 11_100,
        triggerEstimatedInputTokens: 170_000,
        triggerPercent: 85,
      },
    }

    expect(buildTraceAttributeRows(measured)).toEqual(expect.arrayContaining([
      { key: 'conversationTokensBefore', value: '12000' },
      { key: 'reclaimedConversationTokens', value: '11100' },
      { key: 'triggerPercent', value: '85%' },
    ]))
    expect(buildTraceAttributeRows(measured).map((row) => row.key)).not.toContain('attemptRollup')
  })

  it('lists session compactions newest first, including turnless manual ones', () => {
    const manual: RuntimeTraceSpan = {
      ...compaction,
      id: 'span-manual',
      traceId: 'trace-manual',
      turnId: null,
      startedAt: '2026-07-18T01:00:00.000Z',
      endedAt: '2026-07-18T01:00:01.500Z',
      attributes: {
        trigger: 'manual',
        conversationTokensBefore: 8_000,
        conversationTokensAfter: 500,
        reclaimedConversationTokens: 7_500,
      },
    }
    const history = buildCompactionHistory([compaction, manual, model])

    expect(history.map((item) => item.span.id)).toEqual(['span-manual', 'span-compaction'])
    expect(history[0]).toMatchObject({
      trigger: 'manual', turnId: null, durationMs: 1500, reclaimedTokens: 7_500,
    })
    expect(history[1]).toMatchObject({ trigger: 'threshold', turnId: 'turn-1' })
  })

  it('falls back to unknown when a compaction span predates trigger recording', () => {
    const legacy: RuntimeTraceSpan = { ...compaction, attributes: { schemaVersion: 1 } }
    const [item] = buildCompactionHistory([legacy])

    expect(item.trigger).toBe('unknown')
    expect(item.conversationTokensBefore).toBeNull()
    expect(item.reclaimedTokens).toBeNull()
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

  it('classifies every whitelisted attribute and separates each kind primary fields', () => {
    expect(Object.keys(TRACE_ATTRIBUTE_PLACEMENT).sort()).toEqual([...TRACE_ATTRIBUTE_KEYS].sort())

    const modelSections = buildTraceAttributeSections({
      ...model,
      attributes: {
        finishReason: 'stop',
        temperature: 0.2,
        topP: 0.9,
        requestBuildMs: 7,
      },
    })
    expect(modelSections.p0.map((row) => row.key)).toEqual(['temperature', 'finishReason'])
    expect(modelSections.p1.map((row) => row.key)).toEqual(['topP', 'requestBuildMs'])

    const toolSections = buildTraceAttributeSections({
      ...tool,
      attributes: {
        permissionPolicy: 'ask',
        permissionDecision: 'allow',
        executionMs: 12,
      },
    })
    expect(toolSections.p0.map((row) => row.key)).toEqual(['permissionDecision', 'executionMs'])
    expect(toolSections.p1.map((row) => row.key)).toEqual(['permissionPolicy'])

    const compactionSections = buildTraceAttributeSections({
      ...compaction,
      attributes: {
        trigger: 'manual',
        conversationTokensBefore: 1000,
        conversationTokensAfter: 200,
        reclaimedConversationTokens: 800,
        prepareMs: 2,
      },
    })
    expect(compactionSections.p0.map((row) => row.key)).toEqual([
      'trigger',
      'conversationTokensBefore',
      'conversationTokensAfter',
      'reclaimedConversationTokens',
    ])
    expect(compactionSections.p1.map((row) => row.key)).toEqual(['prepareMs'])
  })

  it('keeps semantic scale fields without accepting removed content shadows or attempt details', () => {
    const span = {
      ...model,
      attributes: {
        schemaVersion: 1,
        requestBuildMs: 7,
        ttftMs: 20,
        finishReason: 'stop',
        requestMessageCount: 4,
        toolDefinitionCount: 2,
        // Historical rows may still contain this removed field.
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
      { key: 'toolDefinitionCount', value: '2' },
      { key: 'requestEstimatedSystemContextTokens', value: '20' },
      { key: 'requestEstimatedInputTokens', value: '110' },
    ]))
    expect(rows.map((row) => row.key)).not.toContain('attempts')
    expect(rows.map((row) => row.key)).not.toContain('requestContentBytes')
  })
})
