import { SquareKanban } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { agoText } from '@/features/collab/components/timeText'
import { useNow } from '@/features/collab/components/useNow'
import { elapsedSeconds } from '@/lib/dateTime'
import { cn } from '@/lib/utils'
import type { FoundCard } from './roomTimeline'

/** 正文里的卡片胶囊（collaboration-desktop.md §7.2）；卡片已删除时只显示 id 且不可点击。 */
export function CardChip({ cardId, found, selected, onOpen }: {
  cardId: string
  found: FoundCard | null
  selected: boolean
  onOpen: (cardId: string) => void
}) {
  const { t } = useTranslation()
  if (!found) {
    return <span className="rounded bg-code-bg px-1 font-mono text-[0.85em] text-ink-faint" title={t('collab.rooms.deletedCard')}>{cardId}</span>
  }
  return (
    <button
      type="button"
      aria-label={t('collab.rooms.openCard', { title: found.card.title })}
      className={cn(
        'inline-flex max-w-[260px] items-center gap-1 rounded-full border bg-surface py-px pl-1.5 pr-2 align-[-0.12em] text-[13px] font-semibold text-ink',
        selected ? 'border-clay' : 'border-line-strong',
      )}
      onClick={() => onOpen(cardId)}
    >
      <SquareKanban size={13} className="shrink-0 text-clay" />
      <span className="truncate">{found.card.title}</span>
    </button>
  )
}

/** “多久前更新”的刷新间隔。 */
const AGO_REFRESH_MS = 60_000

/** 消息下方的卡片摘要卡：看板图标、id 前 8 位、标题、“看板 → 列”、负责人、多久前更新。 */
export function CardSummary({ cardId, found, assigneeName, selected, onOpen }: {
  cardId: string
  found: FoundCard
  assigneeName: string | null
  selected: boolean
  onOpen: (cardId: string) => void
}) {
  const { t } = useTranslation()
  const now = useNow(AGO_REFRESH_MS)
  const location = t('collab.rooms.cardLocation', { board: found.board.title, column: found.column.title })
  const ago = found.card.updatedAt ? agoText(elapsedSeconds(found.card.updatedAt, now)) : null
  return (
    <button
      type="button"
      aria-label={t('collab.rooms.openCard', { title: found.card.title })}
      className={cn(
        'mt-1 flex w-[460px] max-w-full items-center gap-3 rounded-xl border bg-surface px-3 py-2.5 text-left text-ink',
        selected ? 'border-clay' : 'border-line-strong',
      )}
      onClick={() => onOpen(cardId)}
    >
      <span aria-hidden="true" className="grid h-11 w-9 shrink-0 place-items-center rounded-md border border-line bg-paper text-clay">
        <SquareKanban size={16} />
      </span>
      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="flex items-center gap-1.5 text-[10.5px] font-bold tracking-widest text-clay">
          {t('collab.rooms.cardLabel')}
          <span className="size-[3px] rounded-full bg-ink-faint" />
          <span className="font-mono font-medium tracking-normal text-ink-faint">{cardId.slice(0, 13)}</span>
        </span>
        <span className="truncate text-sm font-semibold">{found.card.title}</span>
        <span className="truncate text-xs text-ink-soft">
          {[
            location,
            assigneeName ?? t('collab.rooms.unassigned'),
            ...(ago ? [t('collab.boards.updatedAgo', { ago: t(ago.key, ago.values) })] : []),
          ].join(' · ')}
        </span>
      </span>
    </button>
  )
}
