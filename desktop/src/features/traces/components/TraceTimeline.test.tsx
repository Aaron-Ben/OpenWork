import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import type { RuntimeTraceSpan } from '@/bridge/compat'
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
    expect(markup).toContain('left:25%')
    expect(markup).toContain('width:25%')
  })

  it('renders a time ruler and an expanded-by-default collapse toggle on parents', () => {
    const markup = renderToStaticMarkup(
      <TraceTimeline spans={[model, tool]} selectedSpanId={null} onSelect={vi.fn()} />,
    )

    expect(markup).toContain('data-trace-ruler="true"')
    // 有子节点的 model_call 默认展开，可折叠
    expect(markup).toContain('data-collapse-toggle="model-1"')
    expect(markup).toContain('aria-expanded="true"')
    expect(markup).toContain('data-span-id="tool-1"')
  })

  it('groups each model call with its child tools in a numbered timeline card', () => {
    const secondModel = {
      ...model,
      id: 'model-2',
      startedAt: '2026-07-18T00:00:02.500Z',
      endedAt: '2026-07-18T00:00:03.000Z',
    }
    const markup = renderToStaticMarkup(
      <TraceTimeline spans={[model, tool, secondModel]} selectedSpanId="model-1" onSelect={vi.fn()} />,
    )

    expect(markup).toContain('data-trace-group="model-1"')
    expect(markup).toContain('data-trace-group="model-2"')
    expect(markup).toContain('data-trace-sequence="01"')
    expect(markup).toContain('data-trace-sequence="02"')
    expect(markup).toContain('模型')
    expect(markup).toContain('工具')
  })

  it('picks the tool icon by effect and falls back to the wrench for unknown tools', () => {
    const bash = { ...tool, id: 'tool-bash', resolvedToolName: 'bash', requestedToolName: 'bash' }
    const grep = { ...tool, id: 'tool-grep', resolvedToolName: 'grep', requestedToolName: 'grep' }
    const edit = { ...tool, id: 'tool-edit', resolvedToolName: 'edit', requestedToolName: 'edit' }
    const glob = { ...tool, id: 'tool-glob', resolvedToolName: 'glob', requestedToolName: 'glob' }
    const write = { ...tool, id: 'tool-write', resolvedToolName: 'write_file', requestedToolName: 'write_file' }
    const unknown = { ...tool, id: 'tool-x', resolvedToolName: 'teleport', requestedToolName: 'teleport' }
    const markup = renderToStaticMarkup(
      <TraceTimeline spans={[model, tool, bash, grep, edit, glob, write, unknown]} selectedSpanId={null} onSelect={vi.fn()} />,
    )

    expect(markup).toContain('data-tool-icon="read"')
    expect(markup).toContain('data-tool-icon="bash"')
    expect(markup).toContain('data-tool-icon="grep"')
    expect(markup).toContain('data-tool-icon="edit"')
    expect(markup).toContain('data-tool-icon="glob"')
    // write_file 这类带后缀的 resolved 名也按写效果归类
    expect(markup).toContain('data-tool-icon="write"')
    expect(markup).toContain('data-tool-icon="unknown"')
    expect(markup).toMatch(/class="[^"]*lucide-wrench[^"]*"[^>]*data-tool-icon="unknown"/)
  })

  it('renders distinct plan and multi-agent control icons', () => {
    const controlTools = [
      'update_plan',
      'spawn_agent',
      'wait_agent',
      'list_agents',
      'followup_task',
      'interrupt_agent',
    ].map((name, index) => ({
      ...tool,
      id: `control-${index}`,
      providerCallId: `control-${index}`,
      requestedToolName: name,
      resolvedToolName: name,
    }))
    const markup = renderToStaticMarkup(
      <TraceTimeline spans={[model, ...controlTools]} selectedSpanId={null} onSelect={vi.fn()} />,
    )

    const expectedIcons = {
      update_plan: 'lucide-list-todo',
      spawn_agent: 'lucide-bot-message-square',
      wait_agent: 'lucide-timer',
      list_agents: 'lucide-users-round',
      followup_task: 'lucide-message-square-plus',
      interrupt_agent: 'lucide-circle-stop',
    } as const
    for (const [name, iconClass] of Object.entries(expectedIcons)) {
      expect(markup).toMatch(new RegExp(
        `class="[^"]*${iconClass}[^"]*"[^>]*data-tool-icon="${name}"`,
      ))
    }
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
