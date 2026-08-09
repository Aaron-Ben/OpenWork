import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import type { RuntimePermissionRequest } from '@/bridge/compat'
import { ApprovalCardView } from './ApprovalDialog'

const mixedRequest: RuntimePermissionRequest = {
  sessionId: 'session-1',
  turnId: 'turn-1',
  toolCallId: 'tool-1',
  providerCallId: 'call-1',
  toolName: 'bash',
  card: {
    units: [
      {
        display: 'cat /repo/README.md',
        effects: [
          { certainty: 'inferred', effect: { kind: 'read', path: '/repo/README.md' } },
          { certainty: 'readonly_proof', key: 'builtin.allow.workspace_root_read' },
        ],
        verdict: { decision: 'allow', source: 'readonly_proof', ruleId: 'builtin.allow.workspace_root_read' },
        outsideWorkspace: false,
      },
      {
        display: 'cargo test > /tmp/log',
        effects: [
          { certainty: 'trusted_program', program: 'cargo' },
          { certainty: 'inferred', effect: { kind: 'write', path: '/tmp/log' } },
        ],
        verdict: { decision: 'ask', source: 'no_rule_covers', ruleId: null },
        outsideWorkspace: true,
      },
    ],
    raw: 'cat /repo/README.md && cargo test > /tmp/log',
    unparsed: false,
  },
}

const editRequest: RuntimePermissionRequest = {
  ...mixedRequest,
  toolName: 'edit',
  card: {
    units: [{
      display: 'edit /repo/src/timer.ts',
      effects: [{ certainty: 'inferred', effect: { kind: 'write', path: '/repo/src/timer.ts' } }],
      verdict: { decision: 'ask', source: 'no_rule_covers', ruleId: null },
      outsideWorkspace: false,
    }],
    raw: 'edit /repo/src/timer.ts',
    unparsed: false,
  },
}

describe('ApprovalCardView', () => {
  it('summarizes consequences, folds auto-allowed units, relativizes paths, and collapses raw input', () => {
    const markup = renderToStaticMarkup(
      <ApprovalCardView
        request={mixedRequest}
        resolving={false}
        workspaceRoot="/repo"
        onResolve={vi.fn()}
      />,
    )

    expect(markup).toContain('将写到工作区外')
    expect(markup).toContain('1 项只读操作 · cat · 工作区内，已自动放行')
    expect(markup).toContain('README.md')
    expect(markup).not.toContain('只读证明已放行')
    expect(markup).not.toContain('builtin.allow.workspace_root_read</')
    expect(markup).toContain('写到工作区外')
    expect(markup).toContain('查看原始命令')
    expect(markup).toContain('<details class="group border-t')
    expect(markup).toContain('bg-paper-hover')
    expect(markup).not.toContain('bg-ink')
  })

  it('renders one compact write summary instead of repeating command, effect, and verdict', () => {
    const markup = renderToStaticMarkup(
      <ApprovalCardView
        request={editRequest}
        resolving={false}
        workspaceRoot="/repo"
        toolInput={{
          filePath: '/repo/src/timer.ts',
          oldString: 'const delay = 1000',
          newString: 'const delay = interval',
        }}
        onResolve={vi.fn()}
      />,
    )

    expect(markup).toContain('将写入 1 个文件')
    expect(markup).toContain('data-approval-write-summary="true"')
    expect(markup).toContain('>写入</span>')
    expect(markup).toContain('>src/timer.ts</code>')
    expect(markup).toContain('title="/repo/src/timer.ts"')
    expect(markup).toContain('工作区内')
    expect(markup).not.toContain('edit src/timer.ts')
    expect(markup).not.toContain('当前没有规则允许，所以来问你</div>')
    expect(markup).toContain('const delay = 1000')
    expect(markup).toContain('const delay = interval')
    expect(markup).toContain('bg-status-danger-soft')
    expect(markup).toContain('bg-status-success-soft')
  })

  it('reuses the shared file diff renderer for approval previews', () => {
    const markup = renderToStaticMarkup(
      <ApprovalCardView
        request={editRequest}
        resolving={false}
        workspaceRoot="/repo"
        toolInput={{
          filePath: '/repo/src/timer.ts',
          oldString: 'const delay = 1000',
          newString: 'const delay = interval',
        }}
        onResolve={vi.fn()}
      />,
    )

    expect(markup).toContain('data-file-change-code="true"')
    expect(markup).toContain('data-diff-line-number="1"')
    expect(markup).toContain('grid-cols-[3.25rem_1.25rem_minmax(0,1fr)]')
    expect(markup).not.toContain('grid-cols-[4.75rem_minmax(0,1fr)]')
    expect(markup).not.toContain('grid-cols-[2.25rem_2.25rem_1rem_minmax(0,1fr)]')
  })

  it('keeps normal actions on one line with a concise session button and explanation below', () => {
    const sessionRequest: RuntimePermissionRequest = {
      ...editRequest,
      card: {
        ...editRequest.card,
        sessionAction: {
          kind: 'allow_exec',
          grants: [{
            pattern: { kind: 'token_prefix', tokens: ['cargo', 'test'] },
            label: 'cargo test',
            exact: false,
          }],
        },
      },
    }
    const markup = renderToStaticMarkup(
      <ApprovalCardView request={sessionRequest} resolving={false} onResolve={vi.fn()} />,
    )

    expect(markup).toContain('flex flex-nowrap')
    expect(markup).toContain('whitespace-nowrap')
    expect(markup).toContain('允许一次')
    expect(markup).toContain('本会话始终允许')
    expect(markup).toContain('本会话允许以 cargo test 开头的命令')
    expect(markup).toContain('Enter 允许 · Esc 拒绝')
  })

  it('keeps dangerous operations visually distinct and removes the session-wide action', () => {
    const dangerousRequest: RuntimePermissionRequest = {
      ...editRequest,
      toolName: 'bash',
      card: {
        ...editRequest.card,
        raw: 'rm -rf dist',
        sessionAction: {
          kind: 'allow_exec',
          grants: [{ pattern: { kind: 'token_prefix', tokens: ['rm'] }, label: 'rm', exact: false }],
        },
      },
    }
    const markup = renderToStaticMarkup(
      <ApprovalCardView request={dangerousRequest} resolving={false} onResolve={vi.fn()} />,
    )

    expect(markup).toContain('data-approval-danger="true"')
    expect(markup).toContain('border-2 border-status-danger-border')
    expect(markup).toContain('将永久删除文件')
    expect(markup).toContain('取消')
    expect(markup).toContain('仍要删除')
    expect(markup).not.toContain('本会话始终允许')
  })

  it('keeps a huge decision unit scrollable without pushing actions off screen', () => {
    const hugeCommand = [
      "python3 - <<'EOF'",
      ...Array.from({ length: 200 }, (_, index) => `print("line ${index}")`),
      'EOF',
    ].join('\n')
    const hugeRequest: RuntimePermissionRequest = {
      ...editRequest,
      toolName: 'bash',
      card: {
        units: [{
          display: hugeCommand,
          effects: [{ certainty: 'trusted_program', program: 'python3' }],
          verdict: { decision: 'ask', source: 'no_rule_covers', ruleId: null },
          outsideWorkspace: false,
        }],
        raw: hugeCommand,
        unparsed: false,
      },
    }
    const markup = renderToStaticMarkup(
      <ApprovalCardView request={hugeRequest} resolving={false} onResolve={vi.fn()} />,
    )

    expect(markup).toMatch(/data-approval-scroll="true"[^>]*class="[^"]*max-h-\[45vh\]/)
    expect(markup).toMatch(/data-approval-scroll="true"[^>]*class="[^"]*overflow-y-auto/)
    expect(markup).toMatch(/<code class="[^"]*max-h-40[^"]*overflow-y-auto/)
    expect(markup).toContain('print(&quot;line 199&quot;)')
    expect(markup.indexOf('允许一次')).toBeGreaterThan(markup.indexOf('data-approval-scroll'))
  })

  it('keeps the accept-edits scope as secondary copy instead of a button label', () => {
    const modeRequest: RuntimePermissionRequest = {
      ...editRequest,
      card: { ...editRequest.card, sessionAction: { kind: 'enable_accept_edits' } },
    }
    const markup = renderToStaticMarkup(
      <ApprovalCardView request={modeRequest} resolving={false} onResolve={vi.fn()} />,
    )

    expect(markup).toContain('本会话始终允许')
    expect(markup).toContain('工作区内的非敏感写入将不再逐次询问')
    expect(markup).not.toContain('acceptEdits')
  })
})
