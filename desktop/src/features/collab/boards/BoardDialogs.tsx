import { AlertTriangle } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabBoard, CollabBoardColumn } from '@/bridge/collab'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useBoardStore } from './boardStore'

export function BoardEditorDialog({ board, onClose }: { board: CollabBoard | null; onClose: () => void }) {
  const { t } = useTranslation()
  const createBoard = useBoardStore((state) => state.createBoard)
  const updateBoard = useBoardStore((state) => state.updateBoard)
  const [title, setTitle] = useState(board?.title ?? '')
  const [description, setDescription] = useState(board?.description ?? '')
  const [saving, setSaving] = useState(false)

  async function submit(event: React.FormEvent) {
    event.preventDefault()
    const normalizedTitle = title.trim()
    if (!normalizedTitle || saving) return
    setSaving(true)
    try {
      if (board) await updateBoard(board.id, normalizedTitle, optional(description))
      else await createBoard(normalizedTitle, optional(description))
      onClose()
    } catch {
      setSaving(false)
    }
  }

  return (
    <DialogFrame onClose={onClose}>
      <form className="grid gap-3" onSubmit={submit}>
        <h2 className="font-serif text-xl font-semibold">{t(board ? 'collab.boards.editBoard' : 'collab.boards.create')}</h2>
        <Field label={t('collab.boards.name')}><Input autoFocus required maxLength={200} value={title} onChange={(event) => setTitle(event.target.value)} /></Field>
        <Field label={t('collab.boards.description')}><Input value={description} onChange={(event) => setDescription(event.target.value)} /></Field>
        <DialogActions saving={saving} submitLabel={t('common.save')} onClose={onClose} />
      </form>
    </DialogFrame>
  )
}

export function ColumnEditorDialog({ boardId, column, onClose }: { boardId: string; column: CollabBoardColumn | null; onClose: () => void }) {
  const { t } = useTranslation()
  const createColumn = useBoardStore((state) => state.createColumn)
  const updateColumn = useBoardStore((state) => state.updateColumn)
  const [title, setTitle] = useState(column?.title ?? '')
  const [isTerminal, setIsTerminal] = useState(column?.isTerminal ?? false)
  const [saving, setSaving] = useState(false)

  async function submit(event: React.FormEvent) {
    event.preventDefault()
    const normalizedTitle = title.trim()
    if (!normalizedTitle || saving) return
    setSaving(true)
    try {
      if (column) await updateColumn(column.id, normalizedTitle, isTerminal)
      else await createColumn(boardId, normalizedTitle, isTerminal)
      onClose()
    } catch {
      setSaving(false)
    }
  }

  return (
    <DialogFrame onClose={onClose}>
      <form className="grid gap-4" onSubmit={submit}>
        <h2 className="font-serif text-xl font-semibold">{t(column ? 'collab.boards.editColumn' : 'collab.boards.addColumn')}</h2>
        <Field label={t('collab.boards.columnName')}><Input autoFocus required maxLength={120} value={title} onChange={(event) => setTitle(event.target.value)} /></Field>
        <label className="flex cursor-pointer items-start gap-3 rounded-xl border border-line p-3 text-sm">
          <input type="checkbox" className="mt-0.5 size-4 accent-clay" checked={isTerminal} onChange={(event) => setIsTerminal(event.target.checked)} />
          <span>
            <strong className="block font-medium">{t('collab.boards.terminal')}</strong>
            <span className="mt-0.5 block text-xs text-ink-faint">{t('collab.boards.terminalDescription')}</span>
          </span>
        </label>
        <DialogActions saving={saving} submitLabel={t('common.save')} onClose={onClose} />
      </form>
    </DialogFrame>
  )
}

export type DeletableBoardEntity = 'board' | 'column' | 'card'

export function DeleteBoardEntityDialog({ kind, id, label, onClose }: { kind: DeletableBoardEntity; id: string; label: string; onClose: () => void }) {
  const { t } = useTranslation()
  const deleteBoard = useBoardStore((state) => state.deleteBoard)
  const deleteColumn = useBoardStore((state) => state.deleteColumn)
  const deleteCard = useBoardStore((state) => state.deleteCard)
  const [deleting, setDeleting] = useState(false)

  async function confirm() {
    if (deleting) return
    setDeleting(true)
    try {
      if (kind === 'board') await deleteBoard(id)
      else if (kind === 'column') await deleteColumn(id)
      else await deleteCard(id)
      onClose()
    } catch {
      setDeleting(false)
    }
  }

  return (
    <DialogFrame onClose={onClose}>
      <div className="grid gap-4">
        <div className="flex items-start gap-3">
          <span className="grid size-10 shrink-0 place-items-center rounded-full bg-status-danger-soft text-status-danger"><AlertTriangle size={19} /></span>
          <div>
            <h2 className="font-serif text-xl font-semibold">{t(`collab.boards.delete${capitalize(kind)}`)}</h2>
            <p className="mt-1 text-sm text-ink-muted">{t(`collab.boards.delete${capitalize(kind)}Prompt`, { name: label })}</p>
          </div>
        </div>
        <div className="flex justify-end gap-2">
          <Button type="button" variant="ghost" onClick={onClose}>{t('common.cancel')}</Button>
          <Button type="button" variant="destructive" disabled={deleting} onClick={() => void confirm()}>{t('common.delete')}</Button>
        </div>
      </div>
    </DialogFrame>
  )
}

function DialogFrame({ children, onClose }: { children: React.ReactNode; onClose: () => void }) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose() }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [onClose])

  return (
    <div className="fixed inset-0 z-40 grid place-items-center bg-black/30 p-6" role="dialog" aria-modal="true" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose() }}>
      <section className="w-full max-w-md rounded-3xl bg-paper p-6 shadow-xl">{children}</section>
    </div>
  )
}

function DialogActions({ saving, submitLabel, onClose }: { saving: boolean; submitLabel: string; onClose: () => void }) {
  const { t } = useTranslation()
  return (
    <div className="flex justify-end gap-2 pt-2">
      <Button type="button" variant="ghost" onClick={onClose}>{t('common.cancel')}</Button>
      <Button type="submit" variant="accent" disabled={saving}>{submitLabel}</Button>
    </div>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return <label className="grid gap-1 text-xs font-medium text-ink-muted"><span>{label}</span>{children}</label>
}

function optional(value: string): string | null {
  const trimmed = value.trim()
  return trimmed || null
}

function capitalize(value: string): string {
  return `${value[0]?.toUpperCase()}${value.slice(1)}`
}
