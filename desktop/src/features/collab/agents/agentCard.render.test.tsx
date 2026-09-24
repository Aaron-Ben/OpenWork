import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import type { CollabAgent, CollabAgentActivity } from '@/bridge/collab'
import { identityClasses } from '@/features/collab/components/agentIdentity'
import { AgentCard } from './AgentCard'
import { HireCard } from './AgentManager'

const NOW = Date.parse('2026-09-25T10:03:12+08:00')

function render(activity: CollabAgentActivity, archivedAt: string | null = null): string {
  const agent: CollabAgent = {
    id: 'ada', displayName: 'Ada', role: 'Architect', persona: 'Plans migrations.', engineId: 'opencode',
    mainModelId: 'deepseek/deepseek-v4-flash', triageModelId: 'deepseek/deepseek-flash',
    configRevision: 1, agendaEnabled: true, archivedAt, activity,
  }
  const actions = { onEdit: vi.fn(), onArchive: vi.fn(), onAgenda: vi.fn(), onMessage: vi.fn(async () => undefined) }
  return renderToStaticMarkup(<AgentCard agent={agent} now={NOW} actions={actions} />)
}

/** collaboration-desktop.md §13 #13：Agent `activity` 的五种状态各有渲染。 */
describe('AgentCard', () => {
  it('shows the card being worked on with elapsed time', () => {
    const markup = render({
      kind: 'working', roomId: 'room-1', roomTitle: 'Release', cardId: 'card-1', cardTitle: 'Backfill',
      startedAt: '2026-09-25T10:00:00+08:00',
    })
    expect(markup).toContain('>工作中<')
    expect(markup).toContain('处理卡片「Backfill」 · 3 分 12 秒')
    expect(markup).toContain('bg-status-success')
    // 工作中的外圈用 Agent 的识别色（collaboration-desktop.md §11）。
    expect(markup).toContain(identityClasses('ada').ring)
    expect(markup).toContain(identityClasses('ada').avatar)
  })

  it('shows the queued cards', () => {
    const markup = render({ kind: 'queued', cardCount: 2, firstCardTitle: 'Fix login' })
    expect(markup).toContain('>已唤醒<')
    expect(markup).toContain('2 张卡片待处理：「Fix login」')
  })

  it('shows where an idle Agent last spoke', () => {
    const markup = render({ kind: 'idle', roomId: 'room-1', roomTitle: 'Release', lastSpokeAt: '2026-09-25T10:07:00+08:00' })
    expect(markup).toContain('>空闲<')
    expect(markup).toContain('上次在「Release」回复 · 10:07')
  })

  it('shows an error in a danger alert', () => {
    const markup = render({ kind: 'error', message: 'Engine opencode is not ready' })
    expect(markup).toContain('>出错<')
    expect(markup).toContain('role="alert"')
    expect(markup).toContain('Engine opencode is not ready')
  })

  it('offers restore instead of edit for an archived Agent and disables chat and Agenda', () => {
    const markup = render({ kind: 'archived' }, '2026-09-20T10:00:00+08:00')
    expect(markup).toContain('>已归档<')
    expect(markup).toContain('aria-label="恢复 Ada"')
    expect(markup).not.toContain('aria-label="编辑 Ada"')
    expect(markup).not.toContain('role="alert"')
    expect(markup.match(/disabled=""/g)).toHaveLength(2)
  })

  it('shows persona, both models and the Agenda switch', () => {
    const markup = render({ kind: 'idle', roomId: null, roomTitle: null, lastSpokeAt: null })
    expect(markup).toContain('Plans migrations.')
    expect(markup).toContain('deepseek/deepseek-v4-flash')
    expect(markup).toContain('deepseek/deepseek-flash')
    expect(markup).toContain('主动巡检（Agenda）')
    expect(markup).toContain('checked=""')
    expect(markup).toContain('还没有发言')
  })
})

describe('HireCard', () => {
  it('invites creating a new Agent', () => {
    const markup = renderToStaticMarkup(<HireCard onClick={vi.fn()} />)
    expect(markup).toContain('创建同事')
    expect(markup).toContain('设定名字、角色、个性和模型，创建后自动开始接收消息')
  })
})
