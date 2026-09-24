import { SquareKanban, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent } from '@/bridge/collab'
import { MarkdownRenderer } from '@/components/markdown/MarkdownRenderer'
import { useBoardStore } from '@/features/collab/boards/boardStore'
import { useCollabNavigationStore } from '@/features/collab/collabNavigationStore'
import { ParticipantAvatar } from '@/features/collab/components/ParticipantAvatar'
import { findCard } from './roomTimeline'
import { useRoomViewStore } from './roomViewStore'

/**
 * 卡片预览（collaboration-desktop.md §7.5）：只读；“打开看板”跳到看板页并选中这张卡片。
 * 卡片已删除时只显示提示。
 */
export function CardPreviewPanel({ cardId, agents }: { cardId: string, agents: CollabAgent[] }) {
  const { t } = useTranslation()
  const boards = useBoardStore((state) => state.boards)
  const focusCard = useBoardStore((state) => state.focusCard)
  const navigate = useCollabNavigationStore((state) => state.navigate)
  const closePanel = useRoomViewStore((state) => state.closePanel)
  const found = findCard(boards, cardId)
  const assignee = found?.card.assigneeId ? agents.find((agent) => agent.id === found.card.assigneeId) ?? null : null

  return (
    <>
      <div className="flex h-14 shrink-0 items-center gap-2 border-b border-line pl-4 pr-2.5">
        <span className="flex-1 truncate text-xs text-ink-faint">{t('collab.rooms.cardPreview', { board: found?.board.title ?? '' })}</span>
        <button type="button" aria-label={t('collab.rooms.closePreview')} className="grid size-[30px] place-items-center rounded-lg text-ink-soft hover:bg-paper" onClick={closePanel}><X size={15} /></button>
      </div>
      {found ? (
        <>
          <div className="flex flex-1 flex-col gap-3.5 overflow-y-auto p-4">
            <h3 className="font-serif text-lg font-semibold leading-snug">{found.card.title}</h3>
            <div className="grid grid-cols-[60px_minmax(0,1fr)] items-center gap-x-3 gap-y-2.5 text-[13px]">
              <span className="text-ink-faint">{t('collab.rooms.column')}</span>
              <span className="flex items-center gap-1.5">
                {found.column.title}
                {found.column.kind ? <span className="rounded border border-line-strong px-1.5 text-[10px] font-semibold uppercase tracking-wide text-ink-soft">{found.column.kind}</span> : null}
              </span>
              <span className="text-ink-faint">{t('collab.rooms.assignee')}</span>
              <span className="flex items-center gap-2">
                {assignee ? <><ParticipantAvatar name={assignee.displayName} isUser={false} size={20} />{assignee.displayName}</> : t('collab.rooms.unassigned')}
              </span>
            </div>
            {found.card.agentState ? (
              <div className={found.card.agentState === 'working'
                ? 'rounded-xl bg-status-success-soft px-3 py-2.5 text-xs font-semibold text-status-success-ink'
                : 'rounded-xl bg-clay-soft px-3 py-2.5 text-xs font-semibold text-ink'}
              >
                {found.card.agentState === 'working'
                  ? t('collab.rooms.cardWorking', { name: assignee?.displayName ?? '' })
                  : t('collab.rooms.cardQueued')}
              </div>
            ) : null}
            {found.card.description ? (
              <div className="text-[13px] text-ink-soft"><MarkdownRenderer content={found.card.description} variant="compact" /></div>
            ) : null}
          </div>
          <div className="flex justify-end border-t border-line px-4 py-3">
            <button
              type="button"
              className="flex h-8 items-center gap-1.5 rounded-lg border border-line-strong px-3 text-[13px]"
              onClick={() => { focusCard(found.board.id, found.card.id); navigate('boards') }}
            >
              <SquareKanban size={13} className="text-clay" />{t('collab.rooms.openBoard')}
            </button>
          </div>
        </>
      ) : (
        <p className="p-4 text-sm text-ink-faint">{t('collab.rooms.deletedCard')}</p>
      )}
    </>
  )
}
