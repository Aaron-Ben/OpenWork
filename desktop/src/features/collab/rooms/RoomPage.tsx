import { PanelRight } from 'lucide-react'
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent, CollabRoomSummary } from '@/bridge/collab'
import { useBoardStore } from '@/features/collab/boards/boardStore'
import { ParticipantAvatar } from '@/features/collab/components/ParticipantAvatar'
import { cn } from '@/lib/utils'
import { Composer } from './Composer'
import { MessageStream } from './MessageStream'
import { RoomSidebar } from './RoomSidebar'
import { useRoomViewStore } from './roomViewStore'
import { roomWorkers } from './roomWorking'
import { WorkBar } from './WorkBar'

/** 标题栏最多叠放几个成员头像。 */
const HEADER_AVATARS = 4

/**
 * 房间页（collaboration-desktop.md §7）：标题栏、消息流、工作条、输入框与右侧栏。
 * 成员与其当前状态来自房间的 `memberIds` 与 Agent 列表。
 */
export function RoomPage({ room, agents }: { room: CollabRoomSummary, agents: CollabAgent[] }) {
  const { t } = useTranslation()
  const reset = useRoomViewStore((state) => state.reset)
  const quote = useRoomViewStore((state) => state.quote)
  const panelOpen = useRoomViewStore((state) => state.panelOpen)
  const togglePanelOpen = useRoomViewStore((state) => state.togglePanelOpen)
  const fetchBoards = useBoardStore((state) => state.fetchAll)
  const members = agents.filter((agent) => room.memberIds.includes(agent.id))
  const workingCount = roomWorkers(agents, room.id).length

  useEffect(() => { reset() }, [reset, room.id])
  // 卡片胶囊与摘要卡要从看板里查标题（collaboration-desktop.md §7.2）；进入房间页时读取一次。
  useEffect(() => { void fetchBoards() }, [fetchBoards])

  const summary = room.kind === 'group'
    ? t('collab.rooms.groupSummary', { count: members.length })
    : t('collab.rooms.directSummary')

  return (
    <div className="flex min-w-0 flex-1">
      <section className="flex min-w-0 flex-1 flex-col bg-paper">
        <header className="flex h-14 shrink-0 items-center gap-3 border-b border-line px-5">
          <div className="flex min-w-0 flex-1 flex-col">
            <h2 className="truncate font-serif text-lg font-semibold">{room.title ?? room.id}</h2>
            <span className="truncate text-xs text-ink-soft">
              {workingCount > 0 ? `${summary} · ${t('collab.rooms.workingCount', { count: workingCount })}` : summary}
            </span>
          </div>
          <div className="flex" aria-hidden="true">
            {members.slice(0, HEADER_AVATARS).map((agent, index) => (
              <ParticipantAvatar
                key={agent.id}
                participantId={agent.id}
                name={agent.displayName}
                size={28}
                ring={agent.activity.kind === 'working'}
                className={cn(index > 0 && '-ml-1.5 border-2 border-paper')}
              />
            ))}
          </div>
          <button
            type="button"
            aria-label={panelOpen ? t('collab.rooms.hidePanel') : t('collab.rooms.showPanel')}
            aria-pressed={panelOpen}
            className="grid size-[34px] place-items-center rounded-lg border border-line bg-paper-hover text-ink-soft"
            onClick={togglePanelOpen}
          >
            <PanelRight size={16} />
          </button>
        </header>
        <MessageStream roomId={room.id} agents={agents} onQuote={quote} />
        <div className="flex shrink-0 flex-col gap-2 px-6 pb-4">
          <WorkBar agents={agents} roomId={room.id} />
          <Composer roomId={room.id} members={members} />
        </div>
      </section>
      {panelOpen ? <RoomSidebar room={room} members={members} agents={agents} /> : null}
    </div>
  )
}
