import { useEffect } from 'react'

import { listenToCollabEvents } from '@/bridge/collab'
import { useAgentStore } from '@/features/collab/agents/agentStore'
import { useCoordinationStore } from '@/features/collab/coordinationStore'
import { useMessageStore } from '@/features/collab/rooms/messageStore'
import { usePermissionStore } from '@/features/collab/permissions/permissionStore'
import { useRoomStore } from '@/features/collab/rooms/roomStore'
import { resolveErrorMessage } from '@/lib/commandError'
import { createCollabEventController } from './collabEventController'

export function useCollabEventBridge(): void {
  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | null = null
    const refreshAll = async () => {
      await Promise.all([
        useRoomStore.getState().fetchAll(),
        useAgentStore.getState().fetchAll(),
        usePermissionStore.getState().fetchAll(),
      ])
    }
    const controller = createCollabEventController({
      refreshAll,
      refreshRooms: () => useRoomStore.getState().fetchAll(),
      refreshAgents: () => useAgentStore.getState().fetchAll(),
      refreshPermissions: () => usePermissionStore.getState().fetchAll(),
      refreshRoomTail: (roomId) => useMessageStore.getState().refreshTail(roomId),
      applyAgentActivity: (agentId, activity) =>
        useAgentStore.getState().applyActivity(agentId, activity),
      recordHeld: (notice) => useCoordinationStore.getState().recordHeld(notice),
    })
    void refreshAll()
    void listenToCollabEvents((event) => {
      if (!disposed) void controller.process(event)
    }).then((dispose) => {
      if (disposed) dispose()
      else unlisten = dispose
    }).catch((error) => {
      if (!disposed) useRoomStore.setState({ error: resolveErrorMessage(error) })
    })
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [])
}
