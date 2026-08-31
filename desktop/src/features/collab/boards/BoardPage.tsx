import { ClipboardList, Plus } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useBoardStore } from './boardStore'

export function BoardPage() {
  const { t } = useTranslation()
  const boards = useBoardStore((state) => state.boards)
  const runs = useBoardStore((state) => state.runs)
  const error = useBoardStore((state) => state.error)
  const fetchAll = useBoardStore((state) => state.fetchAll)
  const createBoard = useBoardStore((state) => state.createBoard)
  const [creating, setCreating] = useState(false)
  const [title, setTitle] = useState('')

  useEffect(() => {
    void fetchAll()
    const timer = globalThis.setInterval(() => void fetchAll(), 5_000)
    return () => globalThis.clearInterval(timer)
  }, [fetchAll])

  async function submit(event: React.FormEvent) {
    event.preventDefault()
    if (!title.trim()) return
    await createBoard(title.trim())
    setTitle('')
    setCreating(false)
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
            <div>
              <h2 className="font-serif text-xl font-semibold">{board.title}</h2>
              <p className="text-xs text-ink-faint">{board.id}</p>
            </div>
            <div className="grid min-w-[720px] grid-cols-3 gap-3">
              {board.columns.map((column) => (
                <section key={column.id} className="min-h-40 rounded-2xl border border-line bg-paper-hover p-3">
                  <h3 className="mb-3 flex items-center justify-between text-sm font-semibold">
                    <span>{column.title}</span><span className="text-ink-faint">{column.cards.length}</span>
                  </h3>
                  <div className="grid gap-2">
                    {column.cards.map((card) => (
                      <article key={card.id} className="rounded-xl border border-line bg-paper p-3 shadow-sm">
                        <strong className="block text-sm">{card.title}</strong>
                        <span className="mt-1 block text-xs text-ink-faint">
                          {card.assigneeId ? `${t('collab.boards.assignedTo')} @${card.assigneeId}` : t('collab.boards.unassigned')}
                        </span>
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
        <section className="grid gap-2 border-t border-line pt-5">
          <h2 className="font-serif text-lg font-semibold">{t('collab.boards.recentRuns')}</h2>
          {runs.slice(0, 20).map((run) => (
            <article key={run.id} className="rounded-xl border border-line px-3 py-2 text-sm">
              <strong>@{run.agentId}</strong> · {run.engineId} · {run.trigger} · {run.status}{run.outcome ? ` · ${run.outcome}` : ''}
              {run.triggerReason ? <p className="mt-1 text-xs text-ink-faint">{run.triggerReason}</p> : null}
            </article>
          ))}
        </section>
      </div>
      {creating ? (
        <div className="absolute inset-0 z-30 grid place-items-center bg-black/30 p-6" role="dialog" aria-modal="true">
          <form className="grid w-full max-w-md gap-3 rounded-3xl bg-paper p-6 shadow-xl" onSubmit={submit}>
            <h2 className="font-serif text-xl font-semibold">{t('collab.boards.create')}</h2>
            <label className="grid gap-1 text-xs font-medium text-ink-muted">
              <span>{t('collab.boards.name')}</span>
              <Input required value={title} onChange={(event) => setTitle(event.target.value)} />
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
