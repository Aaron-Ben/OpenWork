import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import { coreCommands } from '../../bridge/commands'
import type {
  RuntimeConversationCompaction,
  RuntimeTraceSpan,
  RuntimeTraceSpanPayload,
  RuntimeTraceSummary,
  RuntimeTurnTrace,
} from '../../bridge/compat'
import { isManualCompactionDraft } from '../chat/ChatPage'
import { buildTraceListItems } from './traceViewModel'
import { TraceList } from './components/TraceList'
import {
  initialTraceSpanId,
  loadTraceForSource,
  loadTracePayloadWhenExpanded,
  TracePayloadBody,
} from './components/TurnTraceDrawer'

vi.mock('../../bridge/commands', () => ({
  coreCommands: {
    compactConversation: vi.fn(),
    listTraces: vi.fn(),
    getTraceById: vi.fn(),
    getTrace: vi.fn(),
    getSpanPayload: vi.fn(),
  },
}))

function span(overrides: Partial<RuntimeTraceSpan>): RuntimeTraceSpan {
  return {
    id: 'compact-root', traceId: 'trace-manual', sessionId: 'session-1', turnId: null,
    parentSpanId: null, kind: 'compaction', name: 'session.compact', status: 'succeeded',
    modelId: null, resolvedModelName: 'test-model', providerRequestId: null,
    providerCallId: null, requestedToolName: null, resolvedToolName: null,
    attemptCount: 1, inputTokens: null, outputTokens: null, cachedInputTokens: null,
    reasoningTokens: null, totalTokens: null, responseMessageId: null,
    permissionWaitMs: null, startedAt: '2026-07-27T00:00:00+08:00',
    endedAt: '2026-07-27T00:00:02+08:00', errorCode: null, errorMessage: null,
    attributes: { trigger: 'manual' },
    ...overrides,
  }
}

describe('manual compaction payload flow', () => {
  it('goes from /compact to a clickable turnless Trace and visible summary payload', async () => {
    const compaction: RuntimeConversationCompaction = {
      id: 'checkpoint-1', sessionId: 'session-1', sequence: 1,
      throughMessageSequence: 2, replacedThroughMessageSequence: 0,
      sourceMessageCount: 2, checkpointFormatVersion: 1, kind: 'manual',
      summaryFormatVersion: 1, lastUserMessageId: 'message-user',
      lastUserMessageSequence: 1, resolvedModelName: 'test-model',
      summary: 'summary visible in UI',
      runtimeState: { schemaVersion: 1, editedPaths: [], extensions: {}, warnings: [] },
      runtimeReminderFormatVersion: 1, runtimeReminder: '', triggerTurnId: null,
      parentCompactionId: null, inputTokens: 1_200, outputTokens: 180,
      createdAt: '2026-07-27T00:00:02+08:00',
    }
    const summary: RuntimeTraceSummary = {
      traceId: 'trace-manual', turnId: null, sessionId: 'session-1', turnSequence: null,
      status: 'completed', resolvedModelName: 'test-model', modelCallCount: 0,
      modelSubmissionCount: 0, toolCallCount: 0, spanCount: 2, totalTokens: 1_380,
      startedAt: '2026-07-27T00:00:00+08:00', endedAt: '2026-07-27T00:00:02+08:00',
    }
    const summarySpan = span({
      id: 'summary-model', parentSpanId: 'compact-root', kind: 'model_call',
      name: 'model.call', modelId: 'model-1', inputTokens: 1_200, outputTokens: 180,
      totalTokens: 1_380,
    })
    const trace: RuntimeTurnTrace = {
      summary,
      spans: [span({}), summarySpan],
      completeness: {
        expectedModelCalls: 0, capturedModelCalls: 0, expectedToolCalls: 0,
        capturedToolCalls: 0, orphanToolSpans: 0, runningSpans: 0,
        outcomeUnknownSpans: 0, state: 'complete',
      },
    }
    const payload: RuntimeTraceSpanPayload = {
      spanId: summarySpan.id, slot: 'response', body: { text: 'summary visible in UI' },
      byteSize: 32, truncated: false, originalByteSize: null, redactedCount: 0,
    }
    vi.mocked(coreCommands.compactConversation).mockResolvedValue(compaction)
    vi.mocked(coreCommands.listTraces).mockResolvedValue([summary])
    vi.mocked(coreCommands.getTraceById).mockResolvedValue(trace)
    vi.mocked(coreCommands.getSpanPayload).mockResolvedValue(payload)

    expect(isManualCompactionDraft('/compact')).toBe(true)
    await coreCommands.compactConversation('session-1')
    const summaries = await coreCommands.listTraces('session-1')
    const [item] = buildTraceListItems(summaries, {
      'session-1': { title: 'Manual compaction', workingDirectory: '/repo' },
    }, Date.parse('2026-07-27T00:00:03+08:00'))
    const listMarkup = renderToStaticMarkup(
      <TraceList items={[item]} loading={false} onOpen={vi.fn()} />,
    )
    const loaded = await loadTraceForSource({ kind: 'trace', traceId: item.traceId })
    const selectedSpanId = initialTraceSpanId(loaded, 'trace')
    const loadedPayload = await loadTracePayloadWhenExpanded(
      true,
      selectedSpanId ?? '',
      'response',
    )
    const payloadMarkup = renderToStaticMarkup(<TracePayloadBody payload={loadedPayload!} />)

    expect(listMarkup).toContain('data-trace-row="trace-manual"')
    expect(listMarkup).not.toContain('disabled')
    expect(coreCommands.getTraceById).toHaveBeenCalledWith('trace-manual')
    expect(selectedSpanId).toBe('summary-model')
    expect(coreCommands.getSpanPayload).toHaveBeenCalledWith('summary-model', 'response')
    expect(payloadMarkup).toContain('summary visible in UI')
  })
})
