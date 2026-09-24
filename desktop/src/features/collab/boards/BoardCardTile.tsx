import { Zap } from 'lucide-react'
import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent, CollabCard, CollabColumnKind } from '@/bridge/collab'
import { elapsedText } from '@/features/collab/components/agentStatus'
import { ParticipantAvatar } from '@/features/collab/components/ParticipantAvatar'
import { agoText } from '@/features/collab/components/timeText'
import { textSegments } from '@/features/collab/rooms/messageText'
import { elapsedSeconds } from '@/lib/dateTime'
import { cn } from '@/lib/utils'
import { cardFooter } from './boardModel'

/** 拖动卡片需要的回调；放下由列处理。 */
export interface CardDragHandlers {
  onDragStart: (card: CollabCard) => void
  onDragEnd: () => void
}

/**
 * 看板上的一张卡片（collaboration-desktop.md §9）：标题、描述摘要（`@id` 标出）、负责人与状态。
 * `done` 列的卡片降低不透明度。
 */
export function BoardCardTile({ card, columnKind, agents, now, selected, onSelect, drag }: {
  card: CollabCard
  columnKind: CollabColumnKind | null
  agents: ReadonlyMap<string, CollabAgent>
  now: number
  selected: boolean
  onSelect: () => void
  drag: CardDragHandlers
}) {
  const { t } = useTranslation()
  const assignee = card.assigneeId ? agents.get(card.assigneeId) ?? null : null
  const assigneeName = card.assigneeId === 'local-user' ? t('collab.rooms.user') : assignee?.displayName ?? card.assigneeId
  const knownIds = new Set(agents.keys())
  const ref = useRef<HTMLElement>(null)
  // 从房间“打开看板”选中时滚到可见处（collaboration-desktop.md §7.5）；已经可见时不动。
  useEffect(() => {
    if (selected) ref.current?.scrollIntoView({ block: 'nearest', behavior: 'smooth' })
  }, [selected])

  return (
    <article
      ref={ref}
      data-card-id={card.id}
      draggable
      aria-selected={selected}
      className={cn(
        'flex cursor-grab flex-col gap-2 rounded-xl border bg-paper p-3 shadow-sm active:cursor-grabbing',
        selected ? 'border-clay ring-2 ring-clay/30' : 'border-line hover:border-line-strong',
        columnKind === 'done' && 'opacity-60',
      )}
      tabIndex={0}
      onClick={onSelect}
      onKeyDown={(event) => { if (event.key === 'Enter') onSelect() }}
      onDragStart={(event) => {
        event.dataTransfer.setData('text/plain', card.id)
        event.dataTransfer.effectAllowed = 'move'
        drag.onDragStart(card)
      }}
      onDragEnd={drag.onDragEnd}
    >
      <strong className="text-[13px] font-semibold leading-snug">{card.title}</strong>
      {card.description ? (
        <p className="line-clamp-2 text-xs leading-relaxed text-ink-soft">
          {textSegments(card.description, knownIds).map((segment, index) => segment.kind === 'mention'
            ? <span key={index} className="font-semibold text-clay">@{segment.id}</span>
            : segment.kind === 'card' ? segment.id : segment.text)}
        </p>
      ) : null}
      <CardFooter card={card} assignee={assignee} assigneeName={assigneeName} now={now} />
    </article>
  )
}

function CardFooter({ card, assignee, assigneeName, now }: {
  card: CollabCard
  assignee: CollabAgent | null
  assigneeName: string | null
  now: number
}) {
  const { t } = useTranslation()
  const avatar = assigneeName
    ? <ParticipantAvatar name={assigneeName} isUser={card.assigneeId === 'local-user'} size={20} ring={card.agentState === 'working'} />
    : null
  const footer = cardFooter(card)
  switch (footer) {
    case 'working': {
      const since = assignee?.activity.kind === 'working' ? assignee.activity.startedAt : null
      const elapsed = since ? elapsedText(elapsedSeconds(since, now)) : null
      return (
        <span className="flex items-center gap-1.5 text-xs text-status-success-ink">
          {avatar}
          {elapsed
            ? t('collab.boards.cardWorking', { name: assigneeName, elapsed: t(elapsed.key, elapsed.values) })
            : t('collab.rooms.cardWorking', { name: assigneeName })}
        </span>
      )
    }
    case 'queued':
      return (
        <span className="flex items-center gap-1.5">
          {avatar}
          <span className="flex items-center gap-1 rounded-full bg-clay-soft px-2 py-0.5 text-[11px] font-semibold text-ink"><Zap size={11} />{t('collab.rooms.cardQueued')}</span>
        </span>
      )
    case 'unassigned':
      return <span className="text-xs text-ink-faint">{t('collab.rooms.unassigned')}</span>
    case 'updated': {
      const ago = card.updatedAt ? agoText(elapsedSeconds(card.updatedAt, now)) : null
      return (
        <span className="flex items-center gap-1.5 text-xs text-ink-soft">
          {avatar}
          <span className="truncate">{assigneeName}</span>
          {ago ? <span className="text-ink-faint">· {t('collab.boards.updatedAgo', { ago: t(ago.key, ago.values) })}</span> : null}
        </span>
      )
    }
    default: {
      const unreachable: never = footer
      return unreachable
    }
  }
}
