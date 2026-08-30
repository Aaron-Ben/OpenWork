import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'

import { AgentManager } from '@/features/collab/agents/AgentManager'
import { BoardPage } from '@/features/collab/boards/BoardPage'
import { useAgentStore } from '@/features/collab/agents/agentStore'
import { useCollabNavigationStore } from '@/features/collab/collabNavigationStore'
import { CollabRail } from '@/features/collab/components/CollabRail'
import { MessagePane } from '@/features/collab/rooms/MessagePane'
import { RoomList } from '@/features/collab/rooms/RoomList'
import { useRoomStore } from '@/features/collab/rooms/roomStore'

export function CollabShell() {
  const { t } = useTranslation()
  const view = useCollabNavigationStore((state) => state.view)
  const activeRoomId = useCollabNavigationStore((state) => state.activeRoomId)
  const selectRoom = useCollabNavigationStore((state) => state.selectRoom)
  const rooms = useRoomStore((state) => state.rooms)
  const fetchRooms = useRoomStore((state) => state.fetchAll)
  const fetchAgents = useAgentStore((state) => state.fetchAll)
  const activeRoom = rooms.find((room) => room.id === activeRoomId) ?? null

  useEffect(() => {
    void Promise.all([fetchRooms(), fetchAgents()])
  }, [fetchAgents, fetchRooms])

  useEffect(() => {
    if (!activeRoomId && rooms[0]) selectRoom(rooms[0].id)
  }, [activeRoomId, rooms, selectRoom])

  return (
    <main className="relative flex h-full overflow-hidden bg-paper text-ink">
      <CollabRail view={view} />
      {view === 'rooms' ? (
        <>
          <RoomList rooms={rooms} activeRoomId={activeRoomId} onSelect={selectRoom} />
          {activeRoom ? <MessagePane room={activeRoom} /> : (
            <section data-tauri-drag-region="deep" className="grid min-w-0 flex-1 place-items-center text-sm text-ink-faint">{t('collab.rooms.empty')}</section>
          )}
        </>
      ) : view === 'agents' ? <AgentManager /> : <BoardPage rooms={rooms} />}
    </main>
  )
}
