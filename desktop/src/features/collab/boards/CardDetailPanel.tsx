import { MessagesSquare, Trash2, X, Zap } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent, CollabBoard, CollabCard, CollabCardChange } from '@/bridge/collab'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useCollabNavigationStore } from '@/features/collab/collabNavigationStore'
import { MentionTextarea } from '@/features/collab/components/MentionTextarea'
import { useRoomStore } from '@/features/collab/rooms/roomStore'
import { resolveErrorMessage } from '@/lib/commandError'
import { useBoardStore } from './boardStore'

/** Radix Select 不接受空字符串作为选项值，未分配在界面上用这个值表示。 */
const UNASSIGNED = '__unassigned__'

/**
 * 卡片详情（collaboration-desktop.md §9）：标题与描述直接编辑（失焦或 Cmd+Enter 保存，描述带 `@` 补全），
 * 所在列与负责人，接手规则，当前状态，删除，“在房间中讨论”。保存后提示这次叫醒了谁。
 */
export function CardDetailPanel({ board, card, agents, onDelete }: {
  board: CollabBoard
  card: CollabCard
  agents: CollabAgent[]
  onDelete: () => void
}) {
  const { t } = useTranslation()
  const updateCard = useBoardStore((state) => state.updateCard)
  const moveCard = useBoardStore((state) => state.moveCard)
  const assignCard = useBoardStore((state) => state.assignCard)
  const selectCard = useBoardStore((state) => state.selectCard)
  const openDirect = useRoomStore((state) => state.openDirect)
  const selectRoom = useCollabNavigationStore((state) => state.selectRoom)
  const [title, setTitle] = useState(card.title)
  const [description, setDescription] = useState(card.description ?? '')
  const [notified, setNotified] = useState<string[]>([])
  const [error, setError] = useState<string | null>(null)
  // 最近一次提交的值：Cmd+Enter 保存后紧跟着的失焦不再重复提交。
  const submitted = useRef({ title: card.title, description: card.description ?? '' })
  const active = agents.filter((agent) => agent.archivedAt === null)
  const assignee = agents.find((agent) => agent.id === card.assigneeId) ?? null
  const assigneeName = card.assigneeId === 'local-user' ? t('collab.rooms.user') : assignee?.displayName ?? card.assigneeId

  useEffect(() => {
    setTitle(card.title)
    setDescription(card.description ?? '')
    submitted.current = { title: card.title, description: card.description ?? '' }
  }, [card.id, card.title, card.description])
  useEffect(() => { setNotified([]); setError(null) }, [card.id])

  async function apply(change: () => Promise<CollabCardChange>) {
    setError(null)
    try {
      const result = await change()
      setNotified(result.wokenAgentIds.map((id) => agents.find((agent) => agent.id === id)?.displayName ?? id))
    } catch (changeError) {
      setError(resolveErrorMessage(changeError))
    }
  }

  function saveTitle() {
    const trimmed = title.trim()
    if (!trimmed) { setTitle(card.title); return }
    if (trimmed === submitted.current.title) return
    submitted.current = { ...submitted.current, title: trimmed }
    void apply(() => updateCard(card.id, trimmed, null))
  }

  function saveDescription() {
    if (description === submitted.current.description) return
    submitted.current = { ...submitted.current, description }
    void apply(() => updateCard(card.id, null, description))
  }

  async function discuss() {
    if (!assignee) return
    const room = await openDirect(assignee.id)
    if (room) selectRoom(room.id, t('collab.boards.discussDraft', { title: card.title, id: card.id }))
  }

  return (
    <aside aria-label={t('collab.boards.detail')} className="flex w-[340px] shrink-0 flex-col border-l border-line bg-paper-hover">
      <div className="flex h-14 shrink-0 items-center gap-2 border-b border-line pl-4 pr-2.5">
        <span className="flex-1 truncate text-xs text-ink-soft">{t('collab.rooms.cardPreview', { board: board.title })}</span>
        <button type="button" aria-label={t('collab.boards.closeDetail')} className="grid size-[30px] place-items-center rounded-lg text-ink-soft hover:bg-paper" onClick={() => selectCard(null)}><X size={15} /></button>
      </div>
      <div className="flex flex-1 flex-col gap-4 overflow-y-auto p-4">
        <input
          aria-label={t('collab.boards.cardTitle')}
          value={title}
          className="rounded-lg border border-transparent bg-transparent px-1 font-serif text-lg font-semibold leading-snug outline-none hover:border-line focus:border-clay focus:bg-paper"
          onChange={(event) => setTitle(event.target.value)}
          onBlur={saveTitle}
          onKeyDown={(event) => { if (event.key === 'Enter' && (event.metaKey || event.ctrlKey || !event.shiftKey)) event.currentTarget.blur() }}
        />
        <div className="rounded-xl border border-line bg-paper px-3 py-2">
          <MentionTextarea
            value={description}
            onChange={setDescription}
            agents={active}
            submitKey="mod-enter"
            onSubmit={saveDescription}
            onBlur={saveDescription}
            rows={5}
            popupBelow
            label={t('collab.boards.cardDescription')}
            placeholder={t('collab.boards.descriptionPlaceholder')}
            className="text-[13px] leading-relaxed"
          />
        </div>
        <div className="grid grid-cols-[60px_minmax(0,1fr)] items-center gap-x-3 gap-y-2.5 text-[13px]">
          <span className="text-ink-soft">{t('collab.rooms.column')}</span>
          <Select value={card.columnId} onValueChange={(columnId) => void apply(() => moveCard(card.id, columnId, null))}>
            <SelectTrigger className="h-8 border border-line bg-paper px-2.5 text-[13px]"><SelectValue /></SelectTrigger>
            <SelectContent sideOffset={5}>
              {board.columns.map((column) => <SelectItem key={column.id} value={column.id}>{column.title}</SelectItem>)}
            </SelectContent>
          </Select>
          <span className="text-ink-soft">{t('collab.rooms.assignee')}</span>
          <Select value={card.assigneeId ?? UNASSIGNED} onValueChange={(value) => void apply(() => assignCard(card.id, value === UNASSIGNED ? null : value))}>
            <SelectTrigger className="h-8 border border-line bg-paper px-2.5 text-[13px]"><SelectValue /></SelectTrigger>
            <SelectContent sideOffset={5}>
              <SelectItem value={UNASSIGNED}>{t('collab.rooms.unassigned')}</SelectItem>
              <SelectItem value="local-user">{t('collab.boards.localUser')}</SelectItem>
              {active.map((agent) => <SelectItem key={agent.id} value={agent.id}>{agent.displayName} (@{agent.id})</SelectItem>)}
            </SelectContent>
          </Select>
        </div>
        {card.agentState ? (
          <div className={card.agentState === 'working'
            ? 'rounded-xl bg-status-success-soft px-3 py-2.5 text-xs font-semibold text-status-success-ink'
            : 'flex items-center gap-1.5 rounded-xl bg-clay-soft px-3 py-2.5 text-xs font-semibold text-ink'}
          >
            {card.agentState === 'working' ? t('collab.rooms.cardWorking', { name: assigneeName }) : <><Zap size={12} />{t('collab.rooms.cardQueued')}</>}
          </div>
        ) : null}
        {notified.length > 0 ? (
          <p role="status" className="flex items-center gap-1.5 rounded-xl bg-clay-soft px-3 py-2 text-xs text-ink">
            <Zap size={12} />{t('collab.boards.notified', { names: notified.join(t('collab.rooms.nameSeparator')) })}
          </p>
        ) : null}
        {error ? <p className="text-xs text-status-danger-ink">{error}</p> : null}
        <p className="rounded-xl border border-line bg-paper p-3 text-xs leading-relaxed text-ink-soft">
          {assigneeName ? t('collab.boards.takeoverRule', { name: assigneeName }) : t('collab.boards.takeoverUnassigned')}
        </p>
      </div>
      <div className="flex items-center justify-between gap-2 border-t border-line px-4 py-3">
        <button type="button" className="flex h-8 items-center gap-1.5 rounded-lg px-2.5 text-[13px] text-status-danger-ink hover:bg-status-danger-soft" onClick={onDelete}>
          <Trash2 size={13} />{t('collab.boards.deleteCard')}
        </button>
        <button type="button" disabled={!assignee} className="flex h-8 items-center gap-1.5 rounded-lg border border-line-strong px-3 text-[13px] disabled:opacity-40" onClick={() => void discuss()}>
          <MessagesSquare size={13} />{t('collab.boards.discuss')}
        </button>
      </div>
    </aside>
  )
}
