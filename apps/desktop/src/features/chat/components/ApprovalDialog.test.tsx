import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import type { RuntimePermissionRequest } from '../../../bridge/compat'
import { ApprovalCardView } from './ApprovalDialog'

const request: RuntimePermissionRequest = {
  sessionId: 'session-1',
  turnId: 'turn-1',
  toolCallId: 'tool-1',
  providerCallId: 'call-1',
  toolName: 'bash',
  card: {
    units: [
      {
        display: 'cat README.md',
        effects: [
          {
            certainty: 'inferred',
            effect: { kind: 'read', path: '/repo/README.md' },
          },
          { certainty: 'readonly_proof', key: 'cat' },
        ],
        verdict: { decision: 'allow', source: 'readonly_proof', ruleId: 'builtin.read.workspace' },
        outsideWorkspace: false,
      },
      {
        display: 'cargo test > /tmp/log',
        effects: [
          { certainty: 'trusted_program', program: 'cargo' },
          {
            certainty: 'inferred',
            effect: { kind: 'write', path: '/tmp/log' },
          },
        ],
        verdict: { decision: 'ask', source: 'no_rule_covers', ruleId: null },
        outsideWorkspace: true,
      },
    ],
    raw: 'cat README.md && cargo test > /tmp/log',
    unparsed: false,
  },
}

describe('ApprovalCardView', () => {
  // permissions.md 验收 48/49/50/53: every parsed unit is listed (including
  // the ones a rule already cleared), the raw command stays visible, inferred
  // effects read differently from "trusted program", and out-of-workspace
  // paths are called out.
  it('acc_48_49_50_53 shows every unit, raw input, effect certainty, source, and outside paths', () => {
    const markup = renderToStaticMarkup(
      <ApprovalCardView request={request} resolving={false} onResolve={vi.fn()} />,
    )

    expect(markup).toContain('即将执行 2 项操作')
    expect(markup).toContain('cat README.md')
    expect(markup).toContain('cargo test &gt; /tmp/log')
    expect(markup).toContain('读取 /repo/README.md')
    expect(markup).toContain('只读（已核对参数）：cat')
    expect(markup).toContain('执行（信任该程序）：cargo')
    expect(markup).toContain('写入 /tmp/log')
    expect(markup).toContain('只读证明已放行')
    expect(markup).toContain('无规则覆盖')
    expect(markup).toContain('工作区外')
    expect(markup).toContain('原文')
    expect(markup).toContain('cat README.md &amp;&amp; cargo test &gt; /tmp/log')
  })

  // permissions.md 验收 56: no "always deny", and no button that could
  // produce a persistent rule. The session-scoped middle button arrives in P3.
  it('acc_56 offers only allow-once and deny actions in P1', () => {
    const markup = renderToStaticMarkup(
      <ApprovalCardView request={request} resolving={false} onResolve={vi.fn()} />,
    )

    expect(markup.match(/允许一次/g)).toHaveLength(1)
    expect(markup.match(/拒绝/g)).toHaveLength(1)
    expect(markup).not.toContain('总是拒绝')
    expect(markup).not.toContain('本会话允许')
  })
})
