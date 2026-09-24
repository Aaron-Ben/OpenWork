import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent, CollabRoomSummary } from '@/bridge/collab'
import { useBoardStore } from '@/features/collab/boards/boardStore'
import { useCollabNavigationStore } from '@/features/collab/collabNavigationStore'
import { ParticipantAvatar } from '@/features/collab/components/ParticipantAvatar'
import { useNow } from '@/features/collab/components/useNow'
import { beijingDayStamp } from '@/lib/dateTime'
import { cn } from '@/lib/utils'
import { MessageStream } from './MessageStream'
import { whisperRooms } from './roomListModel'
import { RoomSidebar } from './RoomSidebar'
import { useRoomViewStore } from './roomViewStore'

/** 列表时间的刷新间隔。 */
const STAMP_REFRESH_MS = 60_000

/**
 * “Agent 私聊”页（collaboration-desktop.md §7.6）：只读旁观 Agent 之间的 Direct Room，
 * 没有输入框；打开时同样上报看到的位置。
 */
export function WhispersPage({ rooms, agents }: { rooms: CollabRoomSummary[], agents: CollabAgent[] }) {
  const { t } = useTranslation()
  const activeWhisperId = useCollabNavigationStore((state) => state.activeWhisperId)
  const selectWhisper = useCollabNavigationStore((state) => state.selectWhisper)
  const reset = useRoomViewStore((state) => state.reset)
  const fetchBoards = useBoardStore((state) => state.fetchAll)
  const now = useNow(STAMP_REFRESH_MS)
  const whispers = whisperRooms(rooms)
  const names = new Map(agents.map((agent) => [agent.id, agent.displayName]))
  const active = whispers.find((room) => room.id === activeWhisperId) ?? null

  useEffect(() => { reset() }, [reset, activeWhisperId])
  useEffect(() => { void fetchBoards() }, [fetchBoards])

  const pairTitle = (room: CollabRoomSummary) => {
    const [first = '', second = ''] = room.memberIds.map((id) => names.get(id) ?? id)
    return t('collab.whispers.pairTitle', { first, second })
  }

  return (
    <div className="flex min-w-0 flex-1">
      <aside className="flex w-[272px] shrink-0 flex-col border-r border-line bg-paper-hover">
        <header className="flex h-14 shrink-0 items-center px-4">
          <h1 className="font-serif text-lg font-semibold">{t('collab.whispers.title')}</h1>
        </header>
        <nav className="flex flex-1 flex-col gap-0.5 overflow-y-auto px-2 pb-2">
          {whispers.map((room) => {
            const stamp = room.lastMessageAt ? beijingDayStamp(room.lastMessageAt, now) : null
            return (
              <button
                key={room.id}
                type="button"
                className={cn('flex items-center gap-3 rounded-xl px-2.5 py-2 text-left', room.id === activeWhisperId ? 'bg-paper shadow-sm' : 'hover:bg-paper/60')}
                onClick={() => selectWhisper(room.id)}
              >
                <span className="flex shrink-0">
                  {room.memberIds.slice(0, 2).map((id, index) => (
                    <ParticipantAvatar key={id} name={names.get(id) ?? id} isUser={false} size={28} className={cn(index > 0 && '-ml-2 border-2 border-paper-hover')} />
                  ))}
                </span>
                <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                  <span className="truncate text-sm font-medium">{pairTitle(room)}</span>
                  {room.lastMessage ? (
                    <span className="truncate text-xs text-ink-faint">{t('collab.rooms.lastMessage', { author: room.lastMessage.authorName, body: room.lastMessage.body })}</span>
                  ) : null}
                </span>
                {stamp?.kind === 'today' ? <span className="shrink-0 text-[11px] tabular-nums text-ink-faint">{stamp.clock}</span> : null}
              </button>
            )
          })}
          {whispers.length === 0 ? <p className="px-3 py-8 text-center text-sm text-ink-faint">{t('collab.whispers.empty')}</p> : null}
        </nav>
      </aside>
      {active ? (
        <>
          <section className="flex min-w-0 flex-1 flex-col bg-paper">
            <header className="flex h-14 shrink-0 items-center border-b border-line px-5">
              <h2 className="truncate font-serif text-lg font-semibold">{pairTitle(active)}</h2>
            </header>
            <MessageStream roomId={active.id} agents={agents} onQuote={null} />
            <p className="shrink-0 border-t border-line px-6 py-3 text-center text-xs text-ink-faint">{t('collab.whispers.readOnly')}</p>
          </section>
          <RoomSidebar room={active} members={agents.filter((agent) => active.memberIds.includes(agent.id))} agents={agents} />
        </>
      ) : (
        <section className="grid min-w-0 flex-1 place-items-center bg-paper text-sm text-ink-faint">{whispers.length > 0 ? t('collab.whispers.pickOne') : null}</section>
      )}
    </div>
  )
}
