import { ArrowLeft, ArrowRight, Plus, Unlock } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabBoard, CollabCard, CollabCardInput, CollabRoomSummary } from '@/bridge/collab'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { selectRoomBoards, useBoardStore } from './boardStore'

export function BoardView({ room }: { room: CollabRoomSummary | null }) {
  const { t } = useTranslation()
  const boards = useBoardStore((state) => selectRoomBoards(state.byRoom, room?.id ?? null))
  const loading = useBoardStore((state) => state.loading)
  const error = useBoardStore((state) => state.error)
  const fetchRoom = useBoardStore((state) => state.fetchRoom)
  const createBoard = useBoardStore((state) => state.createBoard)
  const createColumn = useBoardStore((state) => state.createColumn)
  const createCard = useBoardStore((state) => state.createCard)
  const move = useBoardStore((state) => state.move)
  const releaseClaim = useBoardStore((state) => state.releaseClaim)
  const [showBoardForm, setShowBoardForm] = useState(false)
  const [columnBoardId, setColumnBoardId] = useState<string | null>(null)
  const [cardBoardId, setCardBoardId] = useState<string | null>(null)

  useEffect(() => {
    if (room) void fetchRoom(room.id)
  }, [fetchRoom, room])

  if (!room) {
    return <section className="grid min-w-0 flex-1 place-items-center text-sm text-ink-faint">{t('collab.boards.noRoom')}</section>
  }

  const people = new Map(room.members.map((member) => [member.id, member.displayName]))
  const columnBoard = boards.find((board) => board.id === columnBoardId)
  const cardBoard = boards.find((board) => board.id === cardBoardId)
  return (
    <section className="flex min-w-0 flex-1 flex-col overflow-hidden bg-paper" data-collab-board-view={room.id}>
      <header data-tauri-drag-region="deep" className="flex h-12 shrink-0 items-center border-b border-line px-5">
        <div className="min-w-0">
          <h1 className="truncate font-serif text-lg font-semibold">{t('collab.boards.title')}</h1>
          <p className="truncate text-xs text-ink-faint">{room.title ?? room.id}</p>
        </div>
      </header>
      <div className="min-h-0 flex-1 overflow-auto p-5">
        {error ? <p className="mb-3 text-sm text-red-600">{error}</p> : null}
        {loading && boards.length === 0 ? <p className="text-sm text-ink-faint">{t('collab.rooms.loading')}</p> : null}
        {!loading && boards.length === 0 ? <BoardEmptyState onCreate={() => setShowBoardForm(true)} /> : null}
        <div className="grid gap-6">
          {boards.map((board) => (
            <article key={board.id} className="grid gap-3" data-board-id={board.id}>
              <header className="flex items-center justify-between gap-3">
                <h2 className="font-serif text-xl font-semibold">{board.title}</h2>
                <BoardActions
                  boardId={board.id}
                  hasColumns={board.columns.length > 0}
                  onCreateColumn={() => setColumnBoardId(board.id)}
                  onCreateCard={() => setCardBoardId(board.id)}
                />
              </header>
              <div className="flex min-w-max items-start gap-3">
                {board.columns.map((column, columnIndex) => (
                  <section key={column.id} className="w-72 rounded-2xl border border-line bg-paper-hover p-3" data-column-done={column.isDone}>
                    <header className="mb-3 flex items-center justify-between gap-2">
                      <strong className="text-sm">{column.title}</strong>
                      {column.isDone ? <span className="rounded-full bg-green-100 px-2 py-0.5 text-[10px] font-semibold text-green-800">{t('collab.boards.done')}</span> : null}
                    </header>
                    <div className="grid gap-2">
                      {column.cards.map((card) => (
                        <BoardCard
                          key={card.id}
                          card={card}
                          people={people}
                          canMoveLeft={columnIndex > 0}
                          canMoveRight={columnIndex < board.columns.length - 1}
                          onMoveLeft={() => {
                            const target = board.columns[columnIndex - 1]
                            if (target) void move(room.id, card.id, target.id, target.cards.length)
                          }}
                          onMoveRight={() => {
                            const target = board.columns[columnIndex + 1]
                            if (target) void move(room.id, card.id, target.id, target.cards.length)
                          }}
                          onRelease={() => {
                            if (card.claimedBy) void releaseClaim(room.id, card.id, card.claimedBy)
                          }}
                        />
                      ))}
                      {column.cards.length === 0 ? <p className="py-6 text-center text-xs text-ink-faint">{t('collab.boards.noCards')}</p> : null}
                    </div>
                  </section>
                ))}
              </div>
            </article>
          ))}
        </div>
      </div>
      {showBoardForm ? (
        <BoardForm onCancel={() => setShowBoardForm(false)} onCreate={async (title) => {
          if (await createBoard(room.id, title)) setShowBoardForm(false)
        }} />
      ) : null}
      {columnBoard ? (
        <ColumnForm board={columnBoard} onCancel={() => setColumnBoardId(null)} onCreate={async (title, isDone) => {
          if (await createColumn(room.id, columnBoard.id, title, columnBoard.columns.length, isDone)) {
            setColumnBoardId(null)
          }
        }} />
      ) : null}
      {cardBoard ? (
        <CardForm room={room} board={cardBoard} onCancel={() => setCardBoardId(null)} onCreate={async (card) => {
          await createCard(room.id, card)
          setCardBoardId(null)
        }} />
      ) : null}
    </section>
  )
}

export function BoardEmptyState({ onCreate }: { onCreate: () => void }) {
  const { t } = useTranslation()
  return (
    <div className="grid min-h-64 place-items-center text-center">
      <div className="grid gap-3">
        <p className="text-sm text-ink-faint">{t('collab.boards.noBoard')}</p>
        <Button type="button" variant="accent" data-collab-create-board="true" onClick={onCreate}>
          <Plus size={15} />{t('collab.boards.createBoard')}
        </Button>
      </div>
    </div>
  )
}

export function BoardActions({ boardId, hasColumns, onCreateColumn, onCreateCard }: {
  boardId: string
  hasColumns: boolean
  onCreateColumn: () => void
  onCreateCard: () => void
}) {
  const { t } = useTranslation()
  return (
    <div className="flex items-center gap-2">
      <Button type="button" size="sm" variant="outline" data-collab-create-column={boardId} onClick={onCreateColumn}>
        <Plus size={15} />{t('collab.boards.createColumn')}
      </Button>
      <Button type="button" size="sm" disabled={!hasColumns} data-collab-create-card={boardId} onClick={onCreateCard}>
        <Plus size={15} />{t('collab.boards.createCard')}
      </Button>
    </div>
  )
}

export function BoardCard({ card, people, canMoveLeft, canMoveRight, onMoveLeft, onMoveRight, onRelease }: {
  card: CollabCard
  people: ReadonlyMap<string, string>
  canMoveLeft: boolean
  canMoveRight: boolean
  onMoveLeft: () => void
  onMoveRight: () => void
  onRelease: () => void
}) {
  const { t } = useTranslation()
  return (
    <article className="rounded-xl border border-line bg-paper p-3 shadow-sm" data-card-id={card.id}>
      <strong className="block text-sm">{card.title}</strong>
      {card.description ? <p className="mt-1 text-xs leading-relaxed text-ink-muted">{card.description}</p> : null}
      <dl className="mt-3 grid gap-1 text-[11px] text-ink-faint">
        <div className="flex gap-1"><dt>{t('collab.boards.assignee')}:</dt><dd>{card.assigneeId ? people.get(card.assigneeId) ?? card.assigneeId : t('collab.boards.unassigned')}</dd></div>
        {card.claimedBy ? <div className="flex gap-1" data-card-claimant={card.claimedBy}><dt>{t('collab.boards.claimedBy')}:</dt><dd>{people.get(card.claimedBy) ?? card.claimedBy}</dd></div> : null}
      </dl>
      <div className="mt-3 flex items-center justify-end gap-1">
        <Button type="button" variant="ghost" size="icon" className="size-7" disabled={!canMoveLeft} aria-label={t('collab.boards.moveLeft')} onClick={onMoveLeft}><ArrowLeft size={13} /></Button>
        <Button type="button" variant="ghost" size="icon" className="size-7" disabled={!canMoveRight} aria-label={t('collab.boards.moveRight')} onClick={onMoveRight}><ArrowRight size={13} /></Button>
        {card.claimedBy ? <Button type="button" variant="ghost" size="icon" className="size-7 text-red-600" aria-label={t('collab.boards.releaseClaim')} onClick={onRelease}><Unlock size={13} /></Button> : null}
      </div>
    </article>
  )
}

function BoardForm({ onCancel, onCreate }: {
  onCancel: () => void
  onCreate: (title: string) => Promise<void>
}) {
  const { t } = useTranslation()
  const [title, setTitle] = useState('')
  return (
    <ModalForm title={t('collab.boards.createBoard')} onCancel={onCancel} onSubmit={async (event) => {
      event.preventDefault()
      await onCreate(title.trim())
    }}>
      <label className="grid gap-1 text-xs font-medium text-ink-muted">
        <span>{t('collab.boards.boardTitle')}</span>
        <Input required value={title} onChange={(event) => setTitle(event.target.value)} />
      </label>
    </ModalForm>
  )
}

function ColumnForm({ board, onCancel, onCreate }: {
  board: CollabBoard
  onCancel: () => void
  onCreate: (title: string, isDone: boolean) => Promise<void>
}) {
  const { t } = useTranslation()
  const [title, setTitle] = useState('')
  const [isDone, setIsDone] = useState(false)
  return (
    <ModalForm title={t('collab.boards.createColumn')} onCancel={onCancel} onSubmit={async (event) => {
      event.preventDefault()
      await onCreate(title.trim(), isDone)
    }}>
      <p className="text-xs text-ink-faint">{board.title}</p>
      <label className="grid gap-1 text-xs font-medium text-ink-muted">
        <span>{t('collab.boards.columnTitle')}</span>
        <Input required value={title} onChange={(event) => setTitle(event.target.value)} />
      </label>
      <label className="flex items-start gap-3 rounded-xl border border-line p-3 text-sm">
        <input type="checkbox" className="mt-0.5 size-4 accent-clay" checked={isDone} onChange={(event) => setIsDone(event.target.checked)} />
        <span className="grid gap-0.5">
          <strong>{t('collab.boards.doneColumn')}</strong>
          <span className="text-xs text-ink-faint">{t('collab.boards.doneColumnHint')}</span>
        </span>
      </label>
    </ModalForm>
  )
}

function ModalForm({ title, onCancel, onSubmit, children }: {
  title: string
  onCancel: () => void
  onSubmit: (event: React.FormEvent) => Promise<void>
  children: React.ReactNode
}) {
  const { t } = useTranslation()
  return (
    <div className="absolute inset-0 z-30 grid place-items-center bg-black/30 p-6" role="dialog" aria-modal="true">
      <form className="grid w-full max-w-lg gap-3 rounded-3xl bg-paper p-6 shadow-xl" onSubmit={onSubmit}>
        <h2 className="font-serif text-xl font-semibold">{title}</h2>
        {children}
        <div className="flex justify-end gap-2 pt-2">
          <Button type="button" variant="ghost" onClick={onCancel}>{t('common.cancel')}</Button>
          <Button type="submit" variant="accent">{t('collab.agents.save')}</Button>
        </div>
      </form>
    </div>
  )
}

function CardForm({ room, board, onCancel, onCreate }: {
  room: CollabRoomSummary
  board: CollabBoard
  onCancel: () => void
  onCreate: (card: CollabCardInput) => Promise<void>
}) {
  const { t } = useTranslation()
  const firstColumn = board.columns[0]
  const [title, setTitle] = useState('')
  const [description, setDescription] = useState('')
  const [columnId, setColumnId] = useState(firstColumn?.id ?? '')
  const [assigneeId, setAssigneeId] = useState('')
  const agents = useMemo(
    () => room.members.filter((member) => member.kind === 'agent' && member.enabled),
    [room.members],
  )

  async function submit(event: React.FormEvent) {
    event.preventDefault()
    const column = board.columns.find((candidate) => candidate.id === columnId)
    if (!column) return
    await onCreate({
      boardId: board.id,
      columnId,
      title: title.trim(),
      description: description.trim() || null,
      position: column.cards.length,
      assigneeId: assigneeId || null,
    })
  }

  return (
    <div className="absolute inset-0 z-30 grid place-items-center bg-black/30 p-6" role="dialog" aria-modal="true">
      <form className="grid w-full max-w-lg gap-3 rounded-3xl bg-paper p-6 shadow-xl" onSubmit={submit}>
        <h2 className="font-serif text-xl font-semibold">{t('collab.boards.createCard')}</h2>
        <label className="grid gap-1 text-xs font-medium text-ink-muted"><span>{t('collab.boards.cardTitle')}</span><Input required value={title} onChange={(event) => setTitle(event.target.value)} /></label>
        <label className="grid gap-1 text-xs font-medium text-ink-muted"><span>{t('collab.boards.description')}</span><Textarea rows={3} value={description} onChange={(event) => setDescription(event.target.value)} /></label>
        <label className="grid gap-1 text-xs font-medium text-ink-muted"><span>{t('collab.boards.column')}</span><select className="h-9 rounded-lg border border-line bg-paper px-3 text-sm" value={columnId} onChange={(event) => setColumnId(event.target.value)}>{board.columns.map((column) => <option key={column.id} value={column.id}>{column.title}</option>)}</select></label>
        <label className="grid gap-1 text-xs font-medium text-ink-muted"><span>{t('collab.boards.assignee')}</span><select className="h-9 rounded-lg border border-line bg-paper px-3 text-sm" value={assigneeId} onChange={(event) => setAssigneeId(event.target.value)}><option value="">{t('collab.boards.unassigned')}</option>{agents.map((agent) => <option key={agent.id} value={agent.id}>{agent.displayName}</option>)}</select></label>
        <div className="flex justify-end gap-2 pt-2"><Button type="button" variant="ghost" onClick={onCancel}>{t('common.cancel')}</Button><Button type="submit" variant="accent">{t('collab.agents.save')}</Button></div>
      </form>
    </div>
  )
}
