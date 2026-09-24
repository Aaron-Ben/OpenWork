import { ChevronLeft, ChevronRight, Pencil, Trash2 } from 'lucide-react'
import { Fragment, useRef } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent, CollabBoardColumn, CollabColumnKind } from '@/bridge/collab'
import { cn } from '@/lib/utils'
import { AddCardInline } from './AddCardInline'
import { BoardCardTile, type CardDragHandlers } from './BoardCardTile'
import { dropIndexAt } from './boardModel'

/** 列的操作：左右移动、编辑、删除，以及在列底部建卡。 */
export interface ColumnActions {
  onMove: (direction: -1 | 1) => void
  onEdit: () => void
  onDelete: () => void
  onCreateCard: (title: string) => Promise<void>
}

/** 拖动中的状态：正在拖的卡片与目标位置（列与序号）。 */
export interface ColumnDrag extends CardDragHandlers {
  draggingCardId: string | null
  dropIndex: number | null
  onDragOverIndex: (index: number) => void
  onDrop: () => void
}

const KIND_TAG_CLASSES: Record<CollabColumnKind | 'none', string> = {
  todo: 'border border-line-strong text-ink-soft',
  doing: 'bg-status-success-soft text-status-success-ink',
  done: 'bg-ink text-paper',
  none: 'border border-dashed border-line-strong text-ink-faint',
}

/** 列头的类型标记（collaboration-desktop.md §9）：TODO 描边、DOING success 底、DONE 墨色底、未分类虚线。 */
export function KindTag({ kind }: { kind: CollabColumnKind | null }) {
  const { t } = useTranslation()
  const key = kind ?? 'none'
  return (
    <span className={cn('rounded-[5px] px-1.5 py-px text-[10px] font-semibold tracking-wide', KIND_TAG_CLASSES[key])}>
      {t(`collab.boards.kindTags.${key}`)}
    </span>
  )
}

/** 看板上的一列：卡片可以拖进拖出，放下时的位置由 Server 重排。 */
export function BoardColumn({ column, first, last, agents, now, selectedCardId, onSelectCard, actions, drag }: {
  column: CollabBoardColumn
  first: boolean
  last: boolean
  agents: ReadonlyMap<string, CollabAgent>
  now: number
  selectedCardId: string | null
  onSelectCard: (cardId: string) => void
  actions: ColumnActions
  drag: ColumnDrag
}) {
  const { t } = useTranslation()
  const listRef = useRef<HTMLDivElement>(null)
  const dropTarget = drag.draggingCardId !== null && drag.dropIndex !== null

  function onDragOver(event: React.DragEvent) {
    if (drag.draggingCardId === null) return
    event.preventDefault()
    const tiles = listRef.current?.querySelectorAll<HTMLElement>('[data-card-id]') ?? []
    const midpoints = Array.from(tiles, (tile) => {
      const box = tile.getBoundingClientRect()
      return box.top + box.height / 2
    })
    drag.onDragOverIndex(dropIndexAt(midpoints, event.clientY))
  }

  const dropLine = <div aria-hidden="true" className="mx-1.5 h-[3px] rounded-full bg-clay" />

  return (
    <section
      aria-label={column.title}
      data-drop-target={dropTarget || undefined}
      className={cn('flex max-h-full w-72 shrink-0 flex-col rounded-2xl border bg-paper-hover transition-colors', dropTarget ? 'border-clay bg-clay-soft/40' : 'border-line')}
      onDragOver={onDragOver}
      onDrop={(event) => { event.preventDefault(); drag.onDrop() }}
    >
      <div className="group/header flex shrink-0 items-center gap-2 px-3 py-3">
        <h3 className="flex min-w-0 flex-1 items-center gap-1.5 text-sm font-semibold">
          <span className="truncate">{column.title}</span>
          <span className="font-normal text-ink-faint">{column.cards.length}</span>
          <KindTag kind={column.kind} />
        </h3>
        <div className="hidden shrink-0 group-hover/header:flex">
          <IconButton disabled={first} label={t('collab.boards.moveLeft')} onClick={() => actions.onMove(-1)}><ChevronLeft size={13} /></IconButton>
          <IconButton disabled={last} label={t('collab.boards.moveRight')} onClick={() => actions.onMove(1)}><ChevronRight size={13} /></IconButton>
          <IconButton label={t('collab.boards.editColumn')} onClick={actions.onEdit}><Pencil size={13} /></IconButton>
          <IconButton label={t('collab.boards.deleteColumn')} onClick={actions.onDelete}><Trash2 size={13} /></IconButton>
        </div>
      </div>
      <div ref={listRef} className="flex min-h-12 flex-1 flex-col gap-2 overflow-y-auto px-2.5 pb-2">
        {column.cards.map((card, index) => (
          <Fragment key={card.id}>
            {dropTarget && drag.dropIndex === index ? dropLine : null}
            <BoardCardTile
              card={card}
              columnKind={column.kind}
              agents={agents}
              now={now}
              selected={card.id === selectedCardId}
              onSelect={() => onSelectCard(card.id)}
              drag={drag}
            />
          </Fragment>
        ))}
        {dropTarget && drag.dropIndex === column.cards.length ? dropLine : null}
        {column.cards.length === 0 && !dropTarget ? (
          <p className="rounded-xl border border-dashed border-line px-3 py-6 text-center text-xs text-ink-faint">{t('collab.boards.emptyColumn')}</p>
        ) : null}
        <AddCardInline onCreate={actions.onCreateCard} />
      </div>
    </section>
  )
}

function IconButton({ children, disabled = false, label, onClick }: { children: React.ReactNode, disabled?: boolean, label: string, onClick: () => void }) {
  return (
    <button type="button" aria-label={label} title={label} disabled={disabled} className="grid size-7 place-items-center rounded-md text-ink-soft hover:bg-paper hover:text-ink disabled:opacity-30" onClick={onClick}>
      {children}
    </button>
  )
}
