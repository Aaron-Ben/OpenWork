import { CheckCircle2, ChevronLeft, ChevronRight, Pencil, Plus, Trash2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent, CollabBoard, CollabBoardColumn, CollabCard } from '@/bridge/collab'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useBoardStore } from './boardStore'

interface BoardCanvasProps {
  board: CollabBoard
  agents: CollabAgent[]
  onEditBoard: (board: CollabBoard) => void
  onAddColumn: (boardId: string) => void
  onEditColumn: (boardId: string, column: CollabBoardColumn) => void
  onDelete: (kind: 'board' | 'column' | 'card', id: string, label: string) => void
}

const UNASSIGNED = '__unassigned__'

export function BoardCanvas({ board, agents, onEditBoard, onAddColumn, onEditColumn, onDelete }: BoardCanvasProps) {
  const { t } = useTranslation()
  const moveColumn = useBoardStore((state) => state.moveColumn)
  const assignCard = useBoardStore((state) => state.assignCard)

  async function move(index: number, direction: -1 | 1) {
    const column = board.columns[index]
    if (!column) return
    const beforeColumnId = direction < 0
      ? board.columns[index - 1]?.id ?? null
      : board.columns[index + 2]?.id ?? null
    await moveColumn(column.id, beforeColumnId)
  }

  return (
    <main className="flex min-h-0 min-w-0 flex-col overflow-hidden bg-paper">
      <header className="flex shrink-0 items-start justify-between gap-4 border-b border-line px-5 py-4">
        <div className="min-w-0">
          <div className="flex min-w-0 items-center gap-2">
            <h2 className="truncate font-serif text-xl font-semibold">{board.title}</h2>
            <span className="rounded-full bg-paper-hover px-2 py-0.5 font-mono text-[10px] text-ink-faint">{board.id}</span>
          </div>
          {board.description ? <p className="mt-1 text-sm text-ink-muted">{board.description}</p> : null}
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <Button type="button" size="sm" variant="ghost" onClick={() => onAddColumn(board.id)}>
            <Plus size={14} />{t('collab.boards.addColumn')}
          </Button>
          <IconButton label={t('collab.boards.editBoard')} onClick={() => onEditBoard(board)}><Pencil size={14} /></IconButton>
          <IconButton label={t('collab.boards.deleteBoard')} danger onClick={() => onDelete('board', board.id, board.title)}><Trash2 size={14} /></IconButton>
        </div>
      </header>

      <div className="min-h-0 min-w-0 flex-1 overflow-x-auto overflow-y-hidden p-4">
        <div className="flex h-full min-w-max items-start gap-3">
          {board.columns.map((column, index) => (
            <section key={column.id} className="flex max-h-full w-72 shrink-0 flex-col rounded-2xl border border-line bg-paper-hover">
              <div className="flex shrink-0 items-start justify-between gap-2 border-b border-line px-3 py-3">
                <h3 className="flex min-w-0 items-center gap-1.5 text-sm font-semibold">
                  {column.isTerminal ? <CheckCircle2 className="shrink-0 text-status-success" size={14} /> : null}
                  <span className="truncate">{column.title}</span>
                  <span className="text-ink-faint">{column.cards.length}</span>
                </h3>
                <div className="flex shrink-0">
                  <IconButton disabled={index === 0} label={t('collab.boards.moveLeft')} onClick={() => void move(index, -1).catch(() => undefined)}><ChevronLeft size={13} /></IconButton>
                  <IconButton disabled={index === board.columns.length - 1} label={t('collab.boards.moveRight')} onClick={() => void move(index, 1).catch(() => undefined)}><ChevronRight size={13} /></IconButton>
                  <IconButton label={t('collab.boards.editColumn')} onClick={() => onEditColumn(board.id, column)}><Pencil size={13} /></IconButton>
                  <IconButton label={t('collab.boards.deleteColumn')} danger onClick={() => onDelete('column', column.id, column.title)}><Trash2 size={13} /></IconButton>
                </div>
              </div>
              <div className="min-h-0 flex-1 space-y-2 overflow-y-auto p-3">
                {column.cards.map((card) => (
                  <Card
                    key={card.id}
                    card={card}
                    agents={agents}
                    onAssign={(assigneeId) => void assignCard(card.id, assigneeId).catch(() => undefined)}
                    onDelete={() => onDelete('card', card.id, card.title)}
                  />
                ))}
                {column.cards.length === 0 ? (
                  <p className="rounded-xl border border-dashed border-line px-3 py-8 text-center text-xs text-ink-faint">{t('collab.boards.emptyColumn')}</p>
                ) : null}
              </div>
            </section>
          ))}
        </div>
      </div>
    </main>
  )
}

function Card({ card, agents, onAssign, onDelete }: { card: CollabCard; agents: CollabAgent[]; onAssign: (assigneeId: string | null) => void; onDelete: () => void }) {
  const { t } = useTranslation()
  return (
    <article className="rounded-xl border border-line bg-paper p-3 shadow-sm">
      <div className="flex items-start justify-between gap-2">
        <strong className="min-w-0 text-sm leading-5">{card.title}</strong>
        <IconButton label={t('collab.boards.deleteCard')} danger onClick={onDelete}><Trash2 size={13} /></IconButton>
      </div>
      {card.description ? <p className="mt-1 text-xs leading-5 text-ink-muted">{card.description}</p> : null}
      <label className="mt-3 grid gap-1.5 text-[11px] text-ink-faint">
        <span>{t('collab.boards.assignedTo')}</span>
        <Select value={card.assigneeId ?? UNASSIGNED} onValueChange={(value) => onAssign(value === UNASSIGNED ? null : value)}>
          <SelectTrigger className="h-8 w-full border border-line bg-paper-hover px-2.5 text-xs"><SelectValue /></SelectTrigger>
          <SelectContent sideOffset={5}>
            <SelectItem value={UNASSIGNED}>{t('collab.boards.unassigned')}</SelectItem>
            <SelectItem value="local-user">{t('collab.boards.localUser')}</SelectItem>
            {agents.filter((agent) => agent.archivedAt === null).map((agent) => (
              <SelectItem key={agent.id} value={agent.id}>{agent.displayName} (@{agent.id})</SelectItem>
            ))}
          </SelectContent>
        </Select>
      </label>
    </article>
  )
}

function IconButton({ children, disabled = false, danger = false, label, onClick }: { children: React.ReactNode; disabled?: boolean; danger?: boolean; label: string; onClick: () => void }) {
  return (
    <button
      aria-label={label}
      className={`grid size-7 place-items-center rounded-md text-ink-faint transition hover:bg-paper disabled:opacity-30 ${danger ? 'hover:text-status-danger' : 'hover:text-ink'}`}
      disabled={disabled}
      title={label}
      type="button"
      onClick={onClick}
    >
      {children}
    </button>
  )
}
