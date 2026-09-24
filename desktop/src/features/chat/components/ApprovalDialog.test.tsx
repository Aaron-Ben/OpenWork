import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import type { RuntimePermissionRequest } from '@/bridge/compat'
import { ApprovalCardView } from './ApprovalDialog'

/** 与 Rust 侧 `the_card_serializes_as_the_desktop_contract` 同一形状（contracts.md §3）。 */
const escalation: RuntimePermissionRequest = {
  sessionId: 'session-1',
  turnId: 'turn-1',
  toolCallId: 'tool-1',
  providerCallId: 'call-1',
  toolName: 'bash',
  card: {
    mode: 'auto',
    command: 'git push -u origin permission',
    justification: 'Push the branch and set its upstream.',
    paths: [
      { path: '/repo/OpenWork/.git', access: 'write', scope: 'subtree', tier: 'sensitive', inWorkspace: true },
      { path: '/Users/me/.ssh', access: 'read', scope: 'subtree', tier: 'credential', inWorkspace: false },
    ],
    danger: null,
    previousDenial: 'error: could not lock config file .git/config: Operation not permitted',
  },
}

const danger: RuntimePermissionRequest = {
  ...escalation,
  toolCallId: 'tool-2',
  card: {
    mode: 'auto',
    command: 'rm -rf target dist && cargo build --release',
    justification: null,
    paths: [],
    danger: { key: 'rm_recursive_or_force', start: 0, end: 18 },
    previousDenial: null,
  },
}

function render(request: RuntimePermissionRequest): string {
  return renderToStaticMarkup(
    <ApprovalCardView
      request={request}
      resolving={false}
      workspaceRoot="/repo/OpenWork"
      onResolve={vi.fn()}
    />,
  )
}

describe('ApprovalCardView', () => {
  it('lists every escalation path with its access, scope and tier, and the model reason', () => {
    const markup = render(escalation)

    expect(markup).toContain('这条命令需要沙箱外的 2 个路径')
    expect(markup).toContain('bash · 当前模式：自动')
    expect(markup).toContain('git push -u origin permission')
    expect(markup).toContain('Push the branch and set its upstream.')
    expect(markup).toContain('OpenWork/.git')
    expect(markup).toContain('/Users/me/.ssh')
    expect(markup).toContain('整个目录')
    expect(markup).toContain('敏感')
    expect(markup).toContain('凭据')
    expect(markup).toContain('.git/hooks 在任何情况下都不可写。')
    expect(markup).toContain('本轮上一次执行被沙箱拒绝：error: could not lock config file')
    expect(markup).toContain('只作用于这一次：额外允许写 OpenWork/.git、读 /Users/me/.ssh。下一次调用回到“自动”模式。')
  })

  it('offers only allow once and deny, with allow once first for an escalation', () => {
    const markup = render(escalation)
    const buttons = markup.match(/<button[^>]*>.*?<\/button>/g) ?? []

    expect(buttons).toHaveLength(2)
    expect(buttons[0]).toContain('允许一次')
    expect(buttons[1]).toContain('拒绝')
    expect(markup).not.toContain('本会话')
  })

  it('highlights the matched span of a dangerous command and puts deny first', () => {
    const markup = render(danger)
    const buttons = markup.match(/<button[^>]*>.*?<\/button>/g) ?? []

    expect(markup).toContain('这条命令会批量删除文件')
    expect(markup).toMatch(/<mark[^>]*>rm -rf target dist<\/mark>/)
    expect(markup).toContain(' &amp;&amp; cargo build --release')
    expect(markup).toContain('rm 带 -r / -f，会递归删除')
    expect(markup).toContain('rm_recursive_or_force')
    expect(markup).toContain('批准后仍在“自动”模式的沙箱里执行')
    expect(buttons[0]).toContain('拒绝')
    expect(buttons[1]).toContain('允许一次')
    expect(markup).toContain('Enter 拒绝 · Esc 拒绝')
  })

  it('shows both reasons on one card when an escalation is also a dangerous command', () => {
    const markup = render({
      ...danger,
      card: {
        ...danger.card,
        mode: 'accept_edits',
        command: 'rm -rf build && npm run build',
        justification: 'Rebuild from scratch.',
        danger: { key: 'rm_recursive_or_force', start: 0, end: 12 },
        paths: [
          { path: '/repo/OpenWork/build', access: 'write', scope: 'subtree', tier: 'normal', inWorkspace: true },
        ],
      },
    })

    expect(markup).toContain('需要沙箱外的 1 个路径，且会批量删除文件')
    expect(markup).toContain('当前模式：只让编辑工具改文件')
    expect(markup).toContain('工作区')
    expect(markup).toContain('rm_recursive_or_force')
    expect(markup).toContain('一次批准同时覆盖以上两项')
  })

  it('titles a file tool escalation as a write, without a command block', () => {
    const markup = render({
      ...escalation,
      toolName: 'write',
      card: {
        ...escalation.card,
        command: null,
        paths: [{ path: '/repo/OpenWork/.env', access: 'write', scope: 'exact', tier: 'sensitive', inWorkspace: true }],
        previousDenial: null,
      },
    })

    expect(markup).toContain('这次写入需要沙箱外的 1 个路径')
    expect(markup).toContain('单个文件')
    expect(markup).not.toContain('<pre')
  })
})
