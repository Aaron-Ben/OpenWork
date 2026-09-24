import { Pencil, Plus, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent, CollabBoard, CollabBoardColumn, CollabCard } from '@/bridge/collab'
import { Button } from '@/components/ui/button'
import { useNow } from '@/features/collab/components/useNow'
import { BoardColumn } from './BoardColumn'
import { beforeCardIdAt, boardSummary, isNoopMove } from './boardModel'
import { useBoardStore } from './boardStore'
import { CardDetailPanel } from './CardDetailPanel'

interface BoardCanvasProps {
  board: CollabBoard
  agents: CollabAgent[]
  onEditBoard: (board: CollabBoard) => void
  onAddColumn: (boardId: string) => void
  onEditColumn: (boardId: string, column: CollabBoardColumn) => void
  onDelete: (kind: 'board' | 'column' | 'card', id: string, label: string) => void
}

/** 卡片上的已用时间与“多久前更新”每秒刷新。 */
const TICK_MS = 1_000

/** 拖动中：被拖的卡片，以及它悬停在哪一列的第几个位置。 */
interface DragState {
  card: CollabCard
  columnId: string | null
  index: number | null
}

/**
 * 看板（collaboration-desktop.md §9）：标题栏、横向排列的列、右侧卡片详情。卡片可以拖到别的列或同列
 * 别的位置，放下时只发目标列与 `before_card_id`，界面以 Server 返回的结果为准。
 */
export function BoardCanvas({ board, agents, onEditBoard, onAddColumn, onEditColumn, onDelete }: BoardCanvasProps) {
  const { t } = useTranslation()
  const moveColumn = useBoardStore((state) => state.moveColumn)
  const moveCard = useBoardStore((state) => state.moveCard)
  const createCard = useBoardStore((state) => state.createCard)
  const selectedCardId = useBoardStore((state) => state.selectedCardId)
  const selectCard = useBoardStore((state) => state.selectCard)
  const now = useNow(TICK_MS)
  const [drag, setDrag] = useState<DragState | null>(null)
  const agentsById = new Map(agents.map((agent) => [agent.id, agent]))
  const summary = boardSummary(board)
  const selected = board.columns.flatMap((column) => column.cards).find((card) => card.id === selectedCardId) ?? null

  async function move(index: number, direction: -1 | 1) {
    const column = board.columns[index]
    if (!column) return
    const beforeColumnId = direction < 0
      ? board.columns[index - 1]?.id ?? null
      : board.columns[index + 2]?.id ?? null
    await moveColumn(column.id, beforeColumnId)
  }

  function drop(column: CollabBoardColumn) {
    const current = drag
    setDrag(null)
    if (!current || current.columnId !== column.id || current.index === null) return
    const beforeCardId = beforeCardIdAt(column.cards, current.card.id, current.index)
    if (isNoopMove(column.cards, current.card, column.id, beforeCardId)) return
    void moveCard(current.card.id, column.id, beforeCardId).catch(() => undefined)
  }

  return (
    <div className="flex min-h-0 min-w-0 flex-1">
      <main className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-paper">
        <header className="flex shrink-0 items-center gap-4 border-b border-line px-5 py-3.5">
          <div className="flex min-w-0 flex-1 flex-col">
            <h2 className="truncate font-serif text-xl font-semibold">{board.title}</h2>
            <span className="text-xs text-ink-faint">
              {t('collab.boards.summary', { cards: summary.cardCount, agents: summary.workingAgentCount })}
            </span>
          </div>
          <Button type="button" size="sm" variant="ghost" onClick={() => onAddColumn(board.id)}>
            <Plus size={14} />{t('collab.boards.addColumn')}
          </Button>
          <Button type="button" size="icon" variant="ghost" className="size-8" aria-label={t('collab.boards.editBoard')} onClick={() => onEditBoard(board)}><Pencil size={14} /></Button>
          <Button type="button" size="icon" variant="ghost" className="size-8 text-status-danger-ink" aria-label={t('collab.boards.deleteBoard')} onClick={() => onDelete('board', board.id, board.title)}><Trash2 size={14} /></Button>
        </header>
        <div className="min-h-0 min-w-0 flex-1 overflow-x-auto overflow-y-hidden p-4">
          <div className="flex h-full min-w-max items-start gap-3">
            {board.columns.map((column, index) => (
              <BoardColumn
                key={column.id}
                column={column}
                first={index === 0}
                last={index === board.columns.length - 1}
                agents={agentsById}
                now={now}
                selectedCardId={selectedCardId}
                onSelectCard={selectCard}
                actions={{
                  onMove: (direction) => void move(index, direction).catch(() => undefined),
                  onEdit: () => onEditColumn(board.id, column),
                  onDelete: () => onDelete('column', column.id, column.title),
                  onCreateCard: async (title) => {
                    await createCard({ boardId: board.id, columnId: column.id, title, description: null, assigneeId: null })
                  },
                }}
                drag={{
                  draggingCardId: drag?.card.id ?? null,
                  dropIndex: drag?.columnId === column.id ? drag.index : null,
                  onDragStart: (card) => setDrag({ card, columnId: null, index: null }),
                  onDragEnd: () => setDrag(null),
                  onDragOverIndex: (dropIndex) => setDrag((current) => current && (current.columnId !== column.id || current.index !== dropIndex)
                    ? { ...current, columnId: column.id, index: dropIndex }
                    : current),
                  onDrop: () => drop(column),
                }}
              />
            ))}
          </div>
        </div>
      </main>
      {selected ? (
        <CardDetailPanel
          key={selected.id}
          board={board}
          card={selected}
          agents={agents}
          onDelete={() => onDelete('card', selected.id, selected.title)}
        />
      ) : null}
    </div>
  )
}
