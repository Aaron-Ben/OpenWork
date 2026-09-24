import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import type { CollabAgent, CollabBoard, CollabCard } from '@/bridge/collab'
import { AddCardInline } from './AddCardInline'
import { BoardCardTile } from './BoardCardTile'
import { KindTag } from './BoardColumn'
import { CardDetailPanel } from './CardDetailPanel'

const NOW = Date.parse('2026-09-25T10:03:12+08:00')

const ada: CollabAgent = {
  id: 'ada', displayName: 'Ada', role: null, persona: 'p', engineId: 'opencode', mainModelId: 'm', triageModelId: 't',
  configRevision: 1, agendaEnabled: false, archivedAt: null,
  activity: { kind: 'working', roomId: null, roomTitle: null, cardId: 'card-1', cardTitle: 'Backfill', startedAt: '2026-09-25T10:00:00+08:00' },
}
const agents = new Map([['ada', ada]])

function card(overrides: Partial<CollabCard> = {}): CollabCard {
  return {
    id: 'card-1', boardId: 'board-1', columnId: 'col-1', title: 'Backfill', description: null, position: 0,
    assigneeId: 'ada', createdBy: 'local-user', updatedAt: '2026-09-25T09:58:00+08:00', ...overrides,
  }
}

function tile(value: CollabCard, kind: 'todo' | 'doing' | 'done' | null = 'todo'): string {
  const drag = { onDragStart: vi.fn(), onDragEnd: vi.fn() }
  return renderToStaticMarkup(
    <BoardCardTile card={value} columnKind={kind} agents={agents} now={NOW} selected={false} onSelect={vi.fn()} drag={drag} />,
  )
}

/** collaboration-desktop.md §13 #13 的卡片部分与 #14：卡片 `agentState` 两种状态、未分配与最近更新。 */
describe('BoardCardTile', () => {
  it('shows who is working on it and for how long', () => {
    const markup = tile(card({ agentState: 'working' }))
    expect(markup).toContain('Ada 处理中 · 3 分 12 秒')
    expect(markup).toContain('ring-status-success')
  })

  it('shows a queued wake', () => {
    expect(tile(card({ agentState: 'queued' }))).toContain('已唤醒 · 排队')
  })

  it('shows unassigned cards and when an assigned idle card was last updated', () => {
    expect(tile(card({ assigneeId: null }))).toContain('未分配')
    const updated = tile(card())
    expect(updated).toContain('Ada')
    expect(updated).toContain('5 分钟前更新')
  })

  it('highlights mentions in the description, is draggable, and fades in a done column', () => {
    const markup = tile(card({ description: 'ask @ada first' }), 'done')
    expect(markup).toContain('>@ada</span>')
    expect(markup).toContain('draggable="true"')
    expect(markup).toContain('opacity-60')
  })
})

describe('KindTag', () => {
  it('marks todo, doing, done and unsorted columns', () => {
    expect(renderToStaticMarkup(<KindTag kind="todo" />)).toContain('border-line-strong')
    expect(renderToStaticMarkup(<KindTag kind="doing" />)).toContain('bg-status-success-soft')
    expect(renderToStaticMarkup(<KindTag kind="done" />)).toContain('bg-ink')
    const unsorted = renderToStaticMarkup(<KindTag kind={null} />)
    expect(unsorted).toContain('border-dashed')
    expect(unsorted).toContain('未分类')
  })
})

describe('AddCardInline', () => {
  it('starts as an add button at the bottom of the column', () => {
    expect(renderToStaticMarkup(<AddCardInline onCreate={vi.fn()} />)).toContain('添加卡片')
  })
})

describe('CardDetailPanel', () => {
  const board: CollabBoard = {
    id: 'board-1', title: 'Release', description: null, createdBy: 'local-user',
    columns: [{ id: 'col-1', title: 'Todo', position: 0, kind: 'todo', cards: [card()] }],
  }

  it('explains the takeover rule for the assignee and offers to discuss it', () => {
    const markup = renderToStaticMarkup(<CardDetailPanel board={board} card={card({ agentState: 'queued' })} agents={[ada]} onDelete={vi.fn()} />)
    expect(markup).toContain('Ada 正在处理或排队时别的 Agent 领不走；超过 20 分钟没有更新且 Ada 没有在运行，才允许别人接手。')
    expect(markup).toContain('已唤醒 · 排队')
    expect(markup).toContain('value="Backfill"')
    expect(markup).toContain('在房间中讨论')
    expect(markup).not.toMatch(/disabled=""[^>]*>[^<]*<svg[^>]*lucide-messages-square/)
  })

  it('cannot discuss an unassigned card and says anyone may claim it', () => {
    const markup = renderToStaticMarkup(<CardDetailPanel board={board} card={card({ assigneeId: null })} agents={[ada]} onDelete={vi.fn()} />)
    expect(markup).toContain('没有负责人时，任何 Agent 都可以领取这张卡片。')
    expect(markup).toMatch(/<button type="button" disabled=""[^>]*><svg[^>]*lucide-messages-square/)
  })
})
