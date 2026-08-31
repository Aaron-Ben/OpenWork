import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'

import { AgentManager } from '@/features/collab/agents/AgentManager'
import { BoardPage } from '@/features/collab/boards/BoardPage'
import { useAgentStore } from '@/features/collab/agents/agentStore'
import { useCollabNavigationStore } from '@/features/collab/collabNavigationStore'
import { CollabRail } from '@/features/collab/components/CollabRail'
import { ResizableSidebarLayout } from '@/features/collab/components/ResizableSidebarLayout'
import { useCollabInvalidationCoordinator } from '@/features/collab/invalidationCoordinator'
import { MessagePane } from '@/features/collab/rooms/MessagePane'
import { ObservabilityPage } from '@/features/collab/observability/ObservabilityPage'
import { RoomList } from '@/features/collab/rooms/RoomList'
import { useRoomStore } from '@/features/collab/rooms/roomStore'
import { useCollabRuntimeStore } from '@/features/collab/runtimeStore'

export function CollabShell() {
  useCollabInvalidationCoordinator()
  const { t } = useTranslation()
  const view = useCollabNavigationStore((state) => state.view)
  const activeRoomId = useCollabNavigationStore((state) => state.activeRoomId)
  const selectRoom = useCollabNavigationStore((state) => state.selectRoom)
  const rooms = useRoomStore((state) => state.rooms)
  const fetchRooms = useRoomStore((state) => state.fetchAll)
  const agents = useAgentStore((state) => state.agents)
  const fetchAgents = useAgentStore((state) => state.fetchAll)
  const fetchRuntime = useCollabRuntimeStore((state) => state.fetch)
  const activeRoom = rooms.find((room) => room.id === activeRoomId) ?? null

  useEffect(() => {
    void Promise.all([fetchRooms(), fetchAgents(), fetchRuntime()])
  }, [fetchAgents, fetchRooms, fetchRuntime])

  useEffect(() => {
    if (!activeRoomId && rooms[0]) selectRoom(rooms[0].id)
  }, [activeRoomId, rooms, selectRoom])

  return (
    <main className="relative flex h-full overflow-hidden bg-paper text-ink">
      <CollabRail view={view} />
      {view === 'rooms' ? (
        <ResizableSidebarLayout
          className="h-full flex-1"
          storageKey="rooms"
          defaultWidth={272}
          minWidth={224}
          maxWidth={416}
          resizeLabel={t('collab.rooms.resizeSidebar')}
          sidebar={<RoomList rooms={rooms} agents={agents} activeRoomId={activeRoomId} onSelect={selectRoom} />}
        >
          {activeRoom ? <MessagePane room={activeRoom} agents={agents} /> : (
            <section data-tauri-drag-region="deep" className="grid min-w-0 flex-1 place-items-center text-sm text-ink-faint">{t('collab.rooms.empty')}</section>
          )}
        </ResizableSidebarLayout>
      ) : view === 'agents' ? (
        <AgentManager />
      ) : view === 'boards' ? (
        <BoardPage />
      ) : (
        <ObservabilityPage />
      )}
    </main>
  )
}
