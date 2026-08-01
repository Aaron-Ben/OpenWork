import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import type { RuntimeTraceSpan } from '../../../bridge/compat'
import { TraceTimeline } from './TraceTimeline'

const model: RuntimeTraceSpan = {
  id: 'model-1', traceId: 'turn-1', sessionId: 'session-1', turnId: 'turn-1', parentSpanId: null, kind: 'model_call', name: 'model',
  status: 'succeeded', modelId: 'model-1', resolvedModelName: 'deepseek-v4-flash', providerRequestId: 'req-1',
  providerCallId: null, requestedToolName: null, resolvedToolName: null, attemptCount: 2, inputTokens: 10,
  outputTokens: 20, cachedInputTokens: 1, reasoningTokens: 4, totalTokens: 30,
  responseMessageId: null,
  permissionWaitMs: null, startedAt: '2026-07-18T00:00:00.000Z',
  endedAt: '2026-07-18T00:00:02.000Z', errorCode: null, errorMessage: null, attributes: {},
}

const tool: RuntimeTraceSpan = {
  ...model, id: 'tool-1', parentSpanId: 'model-1', kind: 'tool_call', name: 'read', modelId: null,
  resolvedModelName: null, providerRequestId: null, providerCallId: 'call-1', requestedToolName: 'read',
  resolvedToolName: 'read_file', attemptCount: null, inputTokens: null, outputTokens: null,
  cachedInputTokens: null, reasoningTokens: null, totalTokens: null, responseMessageId: null, permissionWaitMs: 120,
  startedAt: '2026-07-18T00:00:00.500Z',
  endedAt: '2026-07-18T00:00:01.000Z',
}

const compaction: RuntimeTraceSpan = {
  ...model,
  id: 'compaction-1',
  kind: 'compaction',
  name: 'session.compact',
  startedAt: '2026-07-18T00:00:00.250Z',
  endedAt: '2026-07-18T00:00:00.750Z',
  attributes: { trigger: 'threshold' },
}

function permissionTool(
  id: string,
  decision: string,
  source: string,
  attributes: Record<string, unknown> = {},
): RuntimeTraceSpan {
  return {
    ...tool,
    id,
    providerCallId: id,
    requestedToolName: id,
    resolvedToolName: id,
    attributes: {
      permissionDecision: decision,
      permissionDecisionSource: source,
      permissionMode: 'default',
      permissionModeOrigin: 'session_default',
      ...attributes,
    },
  }
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

  it('renders compaction as a first-class timeline operation', () => {
    const markup = renderToStaticMarkup(
      <TraceTimeline spans={[compaction, model]} selectedSpanId="compaction-1" onSelect={vi.fn()} />,
    )

    expect(markup).toContain('data-span-id="compaction-1"')
    expect(markup).toContain('Conversation 压缩')
  })

  it('acc_73a_73b distinguishes four permission outcomes and every automatic source', () => {
    const automaticSources = [
      'builtin',
      'readonly_proof',
      'mode',
      'mode_fs_command',
      'session_grant',
    ]
    const spans = [
      model,
      ...automaticSources.map((source) => permissionTool(`auto-${source}`, 'allow', source)),
      permissionTool('silent-denial', 'deny', 'builtin'),
      permissionTool('user-approved', 'allow', 'user'),
      permissionTool('user-denied', 'deny', 'user'),
    ]
    const markup = renderToStaticMarkup(
      <TraceTimeline spans={spans} selectedSpanId={null} onSelect={vi.fn()} />,
    )

    expect(markup.match(/data-permission-activity="auto_allowed"/g)).toHaveLength(5)
    for (const source of automaticSources) {
      expect(markup).toContain(`data-permission-source="${source}"`)
    }
    expect(markup).toContain('data-permission-activity="silently_denied"')
    expect(markup).toContain('data-permission-activity="user_approved"')
    expect(markup).toContain('data-permission-activity="user_denied"')
    for (const label of ['内置规则', '只读证明', 'acceptEdits 模式', '文件系统命令闸门', '会话授权']) {
      expect(markup).toContain(label)
    }
    expect(markup).toContain('静默拒绝')
    expect(markup).toContain('用户批准')
    expect(markup).toContain('用户拒绝')
  })

  it('acc_73c_73d surfaces proof, mode, and mode origin on the permission marker', () => {
    const proved = permissionTool('proved-read', 'allow', 'readonly_proof', {
      readonlyProofKey: 'git status',
      permissionMode: 'accept_edits',
      permissionModeOrigin: 'approval_card',
    })
    const granted = permissionTool('granted-command', 'allow', 'session_grant', {
      permissionRuleId: 'session.approval-call-42.0',
    })
    const markup = renderToStaticMarkup(
      <TraceTimeline spans={[model, proved, granted]} selectedSpanId="proved-read" onSelect={vi.fn()} />,
    )

    expect(markup).toContain('git status')
    expect(markup).toContain('session.approval-call-42.0')
    expect(markup).toContain('接受文件改动')
    expect(markup).toContain('审批卡片')
  })
})
