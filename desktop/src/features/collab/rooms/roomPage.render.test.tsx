import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import type { CollabAgent, CollabAgentActivity, CollabBoard, CollabRoomMessage, CollabRoomSummary } from '@/bridge/collab'
import { identityClasses } from '@/features/collab/components/agentIdentity'
import { Composer, QuoteBar } from './Composer'
import { MessageBody } from './MessageBody'
import { MessageItem, type MessageContext } from './MessageItem'
import { RoomListRow } from './RoomListRow'
import { RoomNoteRow } from './RoomNoteRow'
import { findCard } from './roomTimeline'
import { WorkBar } from './WorkBar'

const CARD = `card-${'3f9a1c2e'.repeat(4)}`
const GONE = `card-${'0'.repeat(32)}`

const board: CollabBoard = {
  id: 'board-1', title: 'Release', description: null, createdBy: 'local-user',
  columns: [{
    id: 'col-1', title: 'Doing', position: 0, kind: 'doing',
    cards: [{ id: CARD, boardId: 'board-1', columnId: 'col-1', title: 'Backfill in batches', description: null, position: 0, assigneeId: 'ada', createdBy: 'ada' }],
  }],
}

function agent(id: string, activity: CollabAgentActivity): CollabAgent {
  return {
    id, displayName: id === 'ada' ? 'Ada' : 'Bo', role: 'Architect', persona: 'p', engineId: 'opencode',
    mainModelId: 'm', triageModelId: 't', configRevision: 1, agendaEnabled: false, archivedAt: null, activity,
  }
}

const idle: CollabAgentActivity = { kind: 'idle', roomId: null, roomTitle: null, lastSpokeAt: null }
const working: CollabAgentActivity = {
  kind: 'working', roomId: 'room-1', roomTitle: 'Release', cardId: CARD, cardTitle: 'Backfill in batches',
  startedAt: new Date(Date.now() - 192_000).toISOString(),
}

function message(overrides: Partial<CollabRoomMessage> = {}): CollabRoomMessage {
  return {
    id: 'msg-2', roomId: 'room-1', sequence: 2, authorId: 'ada', body: 'On it.', quoted: null,
    authorName: 'Ada', authorKind: 'agent', authorRole: 'Architect', createdAt: '2026-09-25T10:07:00+08:00',
    ...overrides,
  }
}

function context(overrides: Partial<MessageContext> = {}): MessageContext {
  return {
    knownIds: new Set(['ada', 'bo']),
    agentNames: new Map([['ada', 'Ada'], ['bo', 'Bo']]),
    findCard: (cardId) => findCard([board], cardId),
    selectedCardId: null,
    highlightedMessageId: null,
    onOpenCard: vi.fn(),
    onOpenAgent: vi.fn(),
    onJump: vi.fn(),
    onQuote: vi.fn(),
    ...overrides,
  }
}

describe('WorkBar', () => {
  /** collaboration-desktop.md §13 #12：工作条出现与消失时高度不变，消息区不跳动。 */
  it('keeps its height when nobody works and names the card when someone does', () => {
    const empty = renderToStaticMarkup(<WorkBar agents={[agent('ada', idle)]} roomId="room-1" />)
    const busy = renderToStaticMarkup(<WorkBar agents={[agent('ada', working)]} roomId="room-1" />)
    expect(empty).toContain('h-[22px]')
    expect(empty).toContain('invisible')
    expect(busy).toContain('h-[22px]')
    expect(busy).not.toContain('invisible')
    expect(busy).toContain('Ada 正在处理「Backfill in batches」 · 3 分 12 秒')
  })
})

describe('MessageBody', () => {
  /** collaboration-desktop.md §13 #12：代码里的卡片 id 不渲染，已删除的卡片不可点击。 */
  it('renders mentions and card chips only outside code, and deleted cards as plain ids', () => {
    const markup = renderToStaticMarkup(
      <MessageBody
        body={`@bo see ${CARD} and \`${CARD}\` and ${GONE}`}
        knownIds={new Set(['bo'])}
        findCard={(cardId) => findCard([board], cardId)}
        selectedCardId={CARD}
        onOpenCard={vi.fn()}
      />,
    )
    expect(markup).toContain('>@bo</span>')
    // 提及标签用被提及者的识别色（collaboration-desktop.md §7.2、§11）。
    expect(markup).toContain(identityClasses('bo').mention)
    expect(markup.match(/aria-label="打开卡片 Backfill in batches"/g)).toHaveLength(1)
    expect(markup).toContain('border-clay')
    expect(markup).toContain(`<code class="`)
    expect(markup).toContain(`title="卡片已删除">${GONE}</span>`)
  })
})

describe('MessageItem', () => {
  it('shows the author, role, clock, a jumpable quote, the card summary and a quote action', () => {
    const markup = renderToStaticMarkup(
      <MessageItem
        message={message({
          body: `Claimed ${CARD}.`,
          quoted: { id: 'msg-1', authorId: 'local-user', authorName: 'User', body: 'Who takes the backfill?' },
        })}
        context={context()}
      />,
    )
    expect(markup).toContain('aria-label="查看 Ada 的资料"')
    expect(markup).toContain(identityClasses('ada').text)
    expect(markup).toContain('Architect')
    expect(markup).toContain('10:07')
    expect(markup).toContain('你：Who takes the backfill?')
    expect(markup).toContain('跳到原文')
    expect(markup).toContain('看板卡片')
    expect(markup).toContain('Release → Doing · Ada')
    expect(markup).toContain('引用回复')
  })

  it('offers no quote reply in a read-only room', () => {
    const markup = renderToStaticMarkup(<MessageItem message={message()} context={context({ onQuote: null })} />)
    expect(markup).not.toContain('引用回复')
  })
})

describe('RoomNoteRow', () => {
  it('explains who stepped aside for a routed message', () => {
    const markup = renderToStaticMarkup(
      <RoomNoteRow note={{ kind: 'routing', afterSequence: 1, skippedNames: ['Ada', 'Cy'], targetNames: ['Bo'] }} />,
    )
    expect(markup).toContain('Ada、Cy 判断这条是给 Bo 的，没有参与')
  })
})

describe('RoomListRow', () => {
  const room: CollabRoomSummary = {
    id: 'room-1', kind: 'group', title: 'Release', unreadCount: 3,
    lastMessage: { authorName: 'Bo', body: 'Index plan' }, lastMessageAt: '2026-09-25T10:05:00+08:00',
    userIsMember: true, memberIds: ['local-user', 'ada', 'bo'], pinned: false,
  }
  const names = new Map([['ada', 'Ada'], ['bo', 'Bo']])
  const now = Date.parse('2026-09-25T12:00:00+08:00')
  const props = { names, active: false, now, onSelect: vi.fn(), onPin: vi.fn(), onManage: vi.fn() }

  /** collaboration-desktop.md §13 #15：群组头像拼图、“正在处理”行、时间与未读数。 */
  it('shows who is working instead of the last message, with time and unread count', () => {
    const busy = renderToStaticMarkup(<RoomListRow room={room} agents={[agent('ada', working)]} {...props} />)
    expect(busy).toContain('Ada 正在处理…')
    expect(busy).not.toContain('Bo：Index plan')
    expect(busy).toContain('>10:05<')
    expect(busy).toContain('>3</span>')
    expect(busy.match(/size-\[19px\]/g)).toHaveLength(3)
    const quiet = renderToStaticMarkup(<RoomListRow room={room} agents={[agent('ada', idle)]} {...props} />)
    expect(quiet).toContain('Bo：Index plan')
  })
})

describe('QuoteBar', () => {
  it('shows who and what is being quoted, with a cancel button', () => {
    const markup = renderToStaticMarkup(<QuoteBar quoting={message({ body: 'Draft ready' })} onCancel={vi.fn()} />)
    expect(markup).toContain('回复 Ada：Draft ready')
    expect(markup).toContain('aria-label="取消引用"')
  })
})

describe('Composer', () => {
  it('shows the mention button and the narrowing hint', () => {
    const markup = renderToStaticMarkup(<Composer roomId="room-1" members={[agent('ada', idle)]} />)
    expect(markup).toContain('aria-label="提及成员"')
    expect(markup).toContain('只 @ 某人时，其他 Agent 会先判断是否与自己有关')
  })
})

describe('mention colours', () => {
  it('uses clay for @all and each Agent’s own colour otherwise', () => {
    const markup = renderToStaticMarkup(
      <MessageBody body="@all and @ada" knownIds={new Set(['ada'])} findCard={() => null} selectedCardId={null} onOpenCard={vi.fn()} />,
    )
    expect(markup).toContain('bg-clay-soft text-clay-ink')
    expect(markup).toContain(identityClasses('ada').mention)
  })
})
