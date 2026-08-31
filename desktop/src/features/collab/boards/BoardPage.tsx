import {
  CheckCircle2,
  ChevronLeft,
  ChevronRight,
  ClipboardList,
  Pencil,
  Plus,
  Trash2,
} from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabBoard, CollabBoardColumn, CollabCard } from '@/bridge/collab'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useBoardStore } from './boardStore'

export function BoardPage() {
  const { t } = useTranslation()
  const boards = useBoardStore((state) => state.boards)
  const agents = useBoardStore((state) => state.agents)
  const error = useBoardStore((state) => state.error)
  const fetchAll = useBoardStore((state) => state.fetchAll)
  const actions = {
    createBoard: useBoardStore((state) => state.createBoard),
    updateBoard: useBoardStore((state) => state.updateBoard),
    deleteBoard: useBoardStore((state) => state.deleteBoard),
    createColumn: useBoardStore((state) => state.createColumn),
    updateColumn: useBoardStore((state) => state.updateColumn),
    moveColumn: useBoardStore((state) => state.moveColumn),
    deleteColumn: useBoardStore((state) => state.deleteColumn),
    assignCard: useBoardStore((state) => state.assignCard),
    deleteCard: useBoardStore((state) => state.deleteCard),
  }
  const [creating, setCreating] = useState(false)
  const [title, setTitle] = useState('')
  const [description, setDescription] = useState('')

  useEffect(() => {
    void fetchAll()
    const timer = globalThis.setInterval(() => void fetchAll(), 5_000)
    return () => globalThis.clearInterval(timer)
  }, [fetchAll])

  async function submit(event: React.FormEvent) {
    event.preventDefault()
    if (!title.trim()) return
    await actions.createBoard(title.trim(), optional(description))
    setTitle('')
    setDescription('')
    setCreating(false)
  }

  async function editBoard(board: CollabBoard) {
    const nextTitle = globalThis.prompt(t('collab.boards.name'), board.title)?.trim()
    if (!nextTitle) return
    const nextDescription = globalThis.prompt(
      t('collab.boards.description'),
      board.description ?? '',
    )
    if (nextDescription === null) return
    await actions.updateBoard(board.id, nextTitle, optional(nextDescription))
  }

  async function addColumn(boardId: string) {
    const columnTitle = globalThis.prompt(t('collab.boards.columnName'))?.trim()
    if (!columnTitle) return
    const isTerminal = globalThis.confirm(t('collab.boards.terminalPrompt'))
    await actions.createColumn(boardId, columnTitle, isTerminal)
  }

  async function editColumn(column: CollabBoardColumn) {
    const nextTitle = globalThis.prompt(t('collab.boards.columnName'), column.title)?.trim()
    if (!nextTitle) return
    const isTerminal = globalThis.confirm(t('collab.boards.terminalPrompt'))
    await actions.updateColumn(column.id, nextTitle, isTerminal)
  }

  async function remove(kind: 'board' | 'column' | 'card', id: string) {
    if (!globalThis.confirm(t(`collab.boards.delete${capitalize(kind)}Prompt`))) return
    if (kind === 'board') await actions.deleteBoard(id)
    if (kind === 'column') await actions.deleteColumn(id)
    if (kind === 'card') await actions.deleteCard(id)
  }

  async function moveColumn(board: CollabBoard, index: number, direction: -1 | 1) {
    const column = board.columns[index]
    if (!column) return
    const beforeColumnId = direction < 0
      ? board.columns[index - 1]?.id ?? null
      : board.columns[index + 2]?.id ?? null
    await actions.moveColumn(column.id, beforeColumnId)
  }

  async function assign(card: CollabCard, assigneeId: string) {
    await actions.assignCard(card.id, assigneeId || null)
  }

  return (
    <section className="min-w-0 flex-1 overflow-y-auto bg-paper">
      <header data-tauri-drag-region="deep" className="flex h-12 items-center justify-between border-b border-line px-6">
        <h1 className="font-serif text-lg font-semibold">{t('collab.boards.title')}</h1>
        <Button type="button" size="sm" onClick={() => setCreating(true)}>
          <Plus size={15} />{t('collab.boards.create')}
        </Button>
      </header>
      <div className="grid gap-6 p-6">
        {error ? <p className="text-sm text-red-600">{error}</p> : null}
        {boards.map((board) => (
          <article key={board.id} className="grid gap-3">
            <div className="flex items-start justify-between gap-3">
              <div>
                <h2 className="font-serif text-xl font-semibold">{board.title}</h2>
                {board.description ? <p className="text-sm text-ink-muted">{board.description}</p> : null}
                <p className="text-xs text-ink-faint">{board.id}</p>
              </div>
              <div className="flex gap-1">
                <Button type="button" size="sm" variant="ghost" onClick={() => void addColumn(board.id)}>
                  <Plus size={14} />{t('collab.boards.addColumn')}
                </Button>
                <IconButton label={t('collab.boards.editBoard')} onClick={() => void editBoard(board)}>
                  <Pencil size={14} />
                </IconButton>
                <IconButton label={t('collab.boards.deleteBoard')} onClick={() => void remove('board', board.id)}>
                  <Trash2 size={14} />
                </IconButton>
              </div>
            </div>
            <div className="flex min-w-[720px] gap-3 overflow-x-auto pb-2">
              {board.columns.map((column, index) => (
                <section key={column.id} className="min-h-40 w-72 shrink-0 rounded-2xl border border-line bg-paper-hover p-3">
                  <div className="mb-3 flex items-start justify-between gap-2">
                    <h3 className="flex min-w-0 items-center gap-1 text-sm font-semibold">
                      {column.isTerminal ? <CheckCircle2 aria-label={t('collab.boards.terminal')} size={14} /> : null}
                      <span className="truncate">{column.title}</span>
                      <span className="text-ink-faint">{column.cards.length}</span>
                    </h3>
                    <div className="flex">
                      <IconButton disabled={index === 0} label={t('collab.boards.moveLeft')} onClick={() => void moveColumn(board, index, -1)}>
                        <ChevronLeft size={13} />
                      </IconButton>
                      <IconButton disabled={index === board.columns.length - 1} label={t('collab.boards.moveRight')} onClick={() => void moveColumn(board, index, 1)}>
                        <ChevronRight size={13} />
                      </IconButton>
                      <IconButton label={t('collab.boards.editColumn')} onClick={() => void editColumn(column)}>
                        <Pencil size={13} />
                      </IconButton>
                      <IconButton label={t('collab.boards.deleteColumn')} onClick={() => void remove('column', column.id)}>
                        <Trash2 size={13} />
                      </IconButton>
                    </div>
                  </div>
                  <div className="grid gap-2">
                    {column.cards.map((card) => (
                      <article key={card.id} className="rounded-xl border border-line bg-paper p-3 shadow-sm">
                        <div className="flex items-start justify-between gap-2">
                          <strong className="block text-sm">{card.title}</strong>
                          <IconButton label={t('collab.boards.deleteCard')} onClick={() => void remove('card', card.id)}>
                            <Trash2 size={13} />
                          </IconButton>
                        </div>
                        {card.description ? <p className="mt-1 text-xs text-ink-muted">{card.description}</p> : null}
                        <label className="mt-2 grid gap-1 text-xs text-ink-faint">
                          <span>{t('collab.boards.assignedTo')}</span>
                          <select
                            className="h-8 rounded-md border border-line bg-paper px-2 text-xs text-ink"
                            value={card.assigneeId ?? ''}
                            onChange={(event) => void assign(card, event.target.value)}
                          >
                            <option value="">{t('collab.boards.unassigned')}</option>
                            <option value="local-user">{t('collab.boards.localUser')}</option>
                            {agents.filter((agent) => agent.archivedAt === null).map((agent) => (
                              <option key={agent.id} value={agent.id}>{agent.displayName} (@{agent.id})</option>
                            ))}
                          </select>
                        </label>
                      </article>
                    ))}
                  </div>
                </section>
              ))}
            </div>
          </article>
        ))}
        {boards.length === 0 ? (
          <div className="grid place-items-center gap-2 py-20 text-sm text-ink-faint">
            <ClipboardList size={28} /><span>{t('collab.boards.empty')}</span>
          </div>
        ) : null}
      </div>
      {creating ? (
        <div className="absolute inset-0 z-30 grid place-items-center bg-black/30 p-6" role="dialog" aria-modal="true">
          <form className="grid w-full max-w-md gap-3 rounded-3xl bg-paper p-6 shadow-xl" onSubmit={submit}>
            <h2 className="font-serif text-xl font-semibold">{t('collab.boards.create')}</h2>
            <label className="grid gap-1 text-xs font-medium text-ink-muted">
              <span>{t('collab.boards.name')}</span>
              <Input required value={title} onChange={(event) => setTitle(event.target.value)} />
            </label>
            <label className="grid gap-1 text-xs font-medium text-ink-muted">
              <span>{t('collab.boards.description')}</span>
              <Input value={description} onChange={(event) => setDescription(event.target.value)} />
            </label>
            <div className="flex justify-end gap-2 pt-2">
              <Button type="button" variant="ghost" onClick={() => setCreating(false)}>{t('common.cancel')}</Button>
              <Button type="submit" variant="accent">{t('collab.boards.create')}</Button>
            </div>
          </form>
        </div>
      ) : null}
    </section>
  )
}

function IconButton({
  children,
  disabled = false,
  label,
  onClick,
}: {
  children: React.ReactNode
  disabled?: boolean
  label: string
  onClick: () => void
}) {
  return (
    <button
      aria-label={label}
      className="grid size-7 place-items-center rounded-md text-ink-faint hover:bg-paper hover:text-ink disabled:opacity-30"
      disabled={disabled}
      title={label}
      type="button"
      onClick={onClick}
    >
      {children}
    </button>
  )
}

function optional(value: string): string | null {
  const trimmed = value.trim()
  return trimmed || null
}

function capitalize(value: string): string {
  return `${value[0]?.toUpperCase()}${value.slice(1)}`
}
