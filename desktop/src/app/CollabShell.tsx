import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { AgentManager } from '@/features/collab/agents/AgentManager'
import { useAgentStore } from '@/features/collab/agents/agentStore'
import { useCollabNavigationStore } from '@/features/collab/collabNavigationStore'
import { BoardView } from '@/features/collab/boards/BoardView'
import { CollabRail } from '@/features/collab/components/CollabRail'
import { usePermissionStore } from '@/features/collab/permissions/permissionStore'
import { LogDrawer } from '@/features/collab/logs/LogDrawer'
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
  const [rosterOpen, setRosterOpen] = useState(true)

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
              <MessagePane
                room={activeRoom}
                rosterOpen={rosterOpen}
                onToggleRoster={() => setRosterOpen((open) => !open)}
              />
              <Roster room={activeRoom} agents={agents} open={rosterOpen} />
            </>
          ) : (
            <section data-tauri-drag-region="deep" className="grid min-w-0 flex-1 place-items-center text-sm text-ink-faint">{t('collab.rooms.empty')}</section>
          )}
        </>
      ) : view === 'agents' ? (
        <AgentManager />
      ) : view === 'boards' ? (
        <BoardView room={activeRoom} />
      ) : (
        <LogDrawer activeRoomId={activeRoomId} />
      )}
    </main>
  )
}
