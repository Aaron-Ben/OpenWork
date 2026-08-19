import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'

import { AgentManager } from '@/features/collab/agents/AgentManager'
import { useAgentStore } from '@/features/collab/agents/agentStore'
import { useCollabNavigationStore } from '@/features/collab/collabNavigationStore'
import { CollabRail } from '@/features/collab/components/CollabRail'
import { usePermissionStore } from '@/features/collab/permissions/permissionStore'
import { MessagePane } from '@/features/collab/rooms/MessagePane'
import { RoomList } from '@/features/collab/rooms/RoomList'
import { Roster } from '@/features/collab/rooms/Roster'
import { totalUnread, useRoomStore } from '@/features/collab/rooms/roomStore'

export function CollabShell() {
  const { t } = useTranslation()
  const view = useCollabNavigationStore((state) => state.view)
  const activeRoomId = useCollabNavigationStore((state) => state.activeRoomId)
  const selectRoom = useCollabNavigationStore((state) => state.selectRoom)
  const rooms = useRoomStore((state) => state.rooms)
  const createRoom = useRoomStore((state) => state.create)
  const agents = useAgentStore((state) => state.agents)
  const permissionCount = usePermissionStore((state) => state.pending.length)
  const activeRoom = rooms.find((room) => room.id === activeRoomId) ?? null

  useEffect(() => {
    if (!activeRoomId && rooms[0]) selectRoom(rooms[0].id)
  }, [activeRoomId, rooms, selectRoom])

  return (
    <main className="relative flex h-full overflow-hidden bg-paper text-ink">
      <CollabRail
        view={view}
        unreadCount={totalUnread(rooms)}
        permissionCount={permissionCount}
      />
      {view === 'rooms' ? (
        <>
          <RoomList rooms={rooms} activeRoomId={activeRoomId} onSelect={selectRoom} onCreate={createRoom} />
          {activeRoom ? (
            <>
              <MessagePane room={activeRoom} />
              <Roster room={activeRoom} agents={agents} />
            </>
          ) : (
            <section data-tauri-drag-region="deep" className="grid min-w-0 flex-1 place-items-center text-sm text-ink-faint">{t('collab.rooms.empty')}</section>
          )}
        </>
      ) : view === 'agents' ? (
        <AgentManager />
      ) : (
        <section data-tauri-drag-region="deep" className="grid min-w-0 flex-1 place-items-center text-sm text-ink-faint">
          {view === 'boards' ? t('collab.unavailable.boards') : t('collab.unavailable.logs')}
        </section>
      )}
    </main>
  )
}
