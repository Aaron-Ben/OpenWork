import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import type { RuntimeTraceSpan } from '@/bridge/compat'
import type { CompactionHistoryItem } from '../traceViewModel'
import { CompactionHistoryList, formatReclaim } from './CompactionHistoryList'

const span: RuntimeTraceSpan = {
  id: 'span-manual', traceId: 'trace-manual', sessionId: 'session-1', turnId: null, parentSpanId: null,
  kind: 'compaction', name: 'session.compact', status: 'succeeded', modelId: 'model-1',
  resolvedModelName: 'deepseek-v4-flash', providerRequestId: null, providerCallId: null,
  requestedToolName: null, resolvedToolName: null, attemptCount: 1, inputTokens: 9_000,
  outputTokens: 800, cachedInputTokens: null, reasoningTokens: null, totalTokens: null,
  responseMessageId: null,
  permissionWaitMs: null, startedAt: '2026-07-25T00:00:00.000Z',
  endedAt: '2026-07-25T00:00:03.000Z', errorCode: null, errorMessage: null, attributes: {},
}

const manual: CompactionHistoryItem = {
  span,
  trigger: 'manual',
  durationMs: 3_000,
  conversationTokensBefore: 12_000,
  conversationTokensAfter: 900,
  reclaimedTokens: 11_100,
  turnId: null,
}

describe('CompactionHistoryList', () => {
  it('shows a turnless manual compaction with what it reclaimed', () => {
    const markup = renderToStaticMarkup(
      <CompactionHistoryList items={[manual]} loading={false} error={null} />,
    )

    expect(markup).toContain('data-compaction-row="span-manual"')
    expect(markup).toContain('手动')
    expect(markup).toContain('无 Turn')
    expect(markup).toContain('12.0k → 900 (−11.1k)')
    expect(markup).toContain('3.00 s')
  })

  it('surfaces the error code of a failed compaction', () => {
    const failed: CompactionHistoryItem = {
      ...manual,
      span: { ...span, id: 'span-failed', status: 'failed', errorCode: 'summary_retries_exhausted' },
      conversationTokensBefore: null,
      conversationTokensAfter: null,
      reclaimedTokens: null,
    }
    const markup = renderToStaticMarkup(
      <CompactionHistoryList items={[failed]} loading={false} error={null} />,
    )

    expect(markup).toContain('summary_retries_exhausted')
    expect(markup).toContain('—')
  })

  it('reports an empty history instead of an empty list', () => {
    const markup = renderToStaticMarkup(
      <CompactionHistoryList items={[]} loading={false} error={null} />,
    )

    expect(markup).toContain('本会话还没有执行过压缩。')
  })

  it('prefers a load failure over the loading placeholder', () => {
    const markup = renderToStaticMarkup(
      <CompactionHistoryList items={[]} loading error="database unavailable" />,
    )

    expect(markup).toContain('database unavailable')
    expect(markup).not.toContain('data-compaction-loading')
  })

  it('derives the reclaim from before and after when the span omits it', () => {
    expect(formatReclaim({ ...manual, reclaimedTokens: null })).toBe('12.0k → 900 (−11.1k)')
  })
})
