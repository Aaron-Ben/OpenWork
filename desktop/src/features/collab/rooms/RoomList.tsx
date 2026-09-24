import { Plus, Search } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent, CollabRoomSummary } from '@/bridge/collab'
import { Button } from '@/components/ui/button'
import { useNow } from '@/features/collab/components/useNow'
import { cn } from '@/lib/utils'
import { CreateGroupDialog } from './CreateGroupDialog'
import { roomListSections, unreadRoomCount, type RoomFilter } from './roomListModel'
import { RoomListRow } from './RoomListRow'
import { useRoomStore } from './roomStore'
import { useRoomViewStore } from './roomViewStore'

const FILTERS: RoomFilter[] = ['all', 'unread', 'direct', 'group']
/** 列表时间（“今天/昨天/日期”）的刷新间隔。 */
const STAMP_REFRESH_MS = 60_000

/** 房间列表（collaboration-desktop.md §7.1）：搜索、四个筛选、置顶在前的平铺列表。 */
export function RoomList({ rooms, agents, activeRoomId, onSelect }: {
  rooms: CollabRoomSummary[]
  agents: CollabAgent[]
  activeRoomId: string | null
  onSelect: (roomId: string) => void
}) {
  const { t } = useTranslation()
  const pin = useRoomStore((state) => state.pin)
  const setManaging = useRoomViewStore((state) => state.setManaging)
  const now = useNow(STAMP_REFRESH_MS)
  const [creating, setCreating] = useState(false)
  const [filter, setFilter] = useState<RoomFilter>('all')
  const [query, setQuery] = useState('')
  const names = new Map(agents.map((agent) => [agent.id, agent.displayName]))
  const { pinned, others } = roomListSections(rooms, agents, filter, query)
  const unread = unreadRoomCount(rooms)

  const row = (room: CollabRoomSummary) => (
    <RoomListRow
      key={room.id}
      room={room}
      agents={agents}
      names={names}
      active={room.id === activeRoomId}
      now={now}
      onSelect={() => onSelect(room.id)}
      onPin={(next) => void pin(room.id, next)}
      onManage={() => { onSelect(room.id); setManaging(true) }}
    />
  )

  return (
    <aside className="flex h-full w-full flex-col border-r border-line bg-paper-hover">
      <header className="flex h-14 shrink-0 items-center justify-between pl-4 pr-3">
        <h1 className="font-serif text-lg font-semibold">{t('collab.rooms.title')}</h1>
        <Button type="button" variant="ghost" size="icon" className="size-8" disabled={agents.filter((agent) => agent.archivedAt === null).length < 2} aria-label={t('collab.rooms.createGroup')} onClick={() => setCreating(true)}>
          <Plus size={16} />
        </Button>
      </header>
      <div className="flex flex-col gap-2.5 px-3 pb-2.5">
        <label className="flex h-9 items-center gap-2 rounded-lg border border-line bg-paper px-2.5 text-sm text-ink-soft">
          <Search size={15} />
          <input type="search" value={query} placeholder={t('collab.rooms.searchPlaceholder')} className="w-full bg-transparent text-ink outline-none" onChange={(event) => setQuery(event.target.value)} />
        </label>
        <div role="group" aria-label={t('collab.rooms.filterLabel')} className="flex flex-wrap gap-1.5">
          {FILTERS.map((item) => (
            <button
              key={item}
              type="button"
              aria-pressed={filter === item}
              className={cn('h-[26px] rounded-full border px-2.5 text-xs font-semibold', filter === item ? 'border-ink bg-ink text-paper' : 'border-line bg-paper text-ink-soft')}
              onClick={() => setFilter(item)}
            >
              {t(`collab.rooms.filters.${item}`)}
              {item === 'unread' && unread > 0 ? <span className="ml-1 opacity-70">{unread}</span> : null}
            </button>
          ))}
        </div>
      </div>
      <nav className="flex flex-1 flex-col gap-0.5 overflow-y-auto px-2 pb-2">
        {pinned.length > 0 ? (
          <>
            <div className="px-2.5 pb-1 pt-1.5 text-[11px] font-semibold tracking-wider text-ink-soft">{t('collab.rooms.pinnedHeading')}</div>
            {pinned.map(row)}
            <div role="separator" className="mx-2.5 my-2 h-px bg-line" />
          </>
        ) : null}
        {others.map(row)}
        {pinned.length + others.length === 0 ? (
          <p className="px-3 py-8 text-center text-sm text-ink-soft">{rooms.length === 0 ? t('collab.rooms.empty') : t('collab.rooms.noMatches')}</p>
        ) : null}
      </nav>
      {creating ? (
        <CreateGroupDialog
          agents={agents}
          onClose={() => setCreating(false)}
          onCreated={(roomId) => { setCreating(false); onSelect(roomId) }}
        />
      ) : null}
    </aside>
  )
}
